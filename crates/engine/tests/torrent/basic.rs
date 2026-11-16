use crate::helpers::{complete, make_info, shared_file};
use std::time::Duration;
use swiftget_engine::bt::{Live, Torrents};
use swiftget_engine::torrent::AddOptions;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_torrent_and_goes_on_after_a_restart() {
    let Some((root, data, file, seeder, sg, _)) = shared_file("rpc").await else { return };
    let get_dir = root.join("get");
    let getter = Torrents::new(&root.join("b"));
    let opts = AddOptions { dir: get_dir.clone(), ..Default::default() };
    let gg = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&getter, &gg, 60).await, "the torrent should finish");
    assert_eq!(std::fs::read(get_dir.join("song.bin")).unwrap(), data);
    let files = getter.files(&gg).await.unwrap();
    assert_eq!(files[0].path, get_dir.join("song.bin"));
    assert_eq!(files[0].done, data.len() as u64);
    let st = getter.status(&gg).await.unwrap();
    assert!(st.pieces > 0 && swiftget_engine::torrent::piece_runs(&st.bitfield, st.pieces, 100).iter().all(|r| r.2), "{st:?}");
    // the people: the one that shares it
    let _ = getter.peers(&gg).await.unwrap();
    // a new tracker while it runs: started again, nothing checked from nothing
    let live = Live { trackers: vec!["udp://tracker.test:1/announce".into()], ..Default::default() };
    getter.change(&gg, &live).await.unwrap();
    assert!(getter.status(&gg).await.unwrap().trackers.iter().any(|t| t == "udp://tracker.test:1/announce"));
    assert!(complete(&getter, &gg, 30).await);
    // stopped, the program closed and opened again: it goes on from its notes
    getter.remove(&gg).await;
    drop(getter);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let getter = Torrents::new(&root.join("b"));
    let gg2 = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert_eq!(gg, gg2);
    assert!(complete(&getter, &gg2, 30).await);
    // its file deleted while it was stopped: downloaded again, not taken as there
    getter.remove(&gg2).await;
    std::fs::remove_file(get_dir.join("song.bin")).unwrap();
    let gg3 = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&getter, &gg3, 60).await);
    assert_eq!(std::fs::read(get_dir.join("song.bin")).unwrap(), data);
    getter.forget(&gg3).await;
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn torrent_in_the_download_list() {
    let Some((root, data, file, seeder, sg, _)) = shared_file("list").await else { return };
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let list_dir = root.join("list");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(list_dir.clone()),
        start: true,
        ..Default::default()
    });
    let v = mgr.get(id).unwrap();
    assert_eq!(v.kind, swiftget_engine::DownloadKind::Torrent);
    assert_eq!(v.file_name, "song.bin");
    assert_eq!(v.total_size, Some(data.len() as u64));
    let mut status = v.status;
    for _ in 0..600 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let v = mgr.get(id).unwrap();
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", v.error);
    assert_eq!(std::fs::read(list_dir.join("song.bin")).unwrap(), data);
    // the same torrent again into the same folder: the file there is checked, not refused
    let again = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(list_dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut st2 = swiftget_engine::Status::Queued;
    for _ in 0..600 {
        st2 = mgr.get(again).unwrap().status;
        if matches!(st2, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(st2, swiftget_engine::Status::Completed, "{:?}", mgr.get(again).unwrap().error);
    mgr.remove(again, false);
    // delete with the file: exactly the torrent's file goes
    std::fs::write(list_dir.join("mine.txt"), b"keep").unwrap();
    mgr.remove(id, true);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!list_dir.join("song.bin").exists());
    assert!(list_dir.join("mine.txt").exists());
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_link_learns_its_name() {
    let Some((root, data, _file, seeder, sg, port)) = shared_file("magnet").await else { return };
    let hash: String = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &make_info("song.bin", &data))
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http%3A%2F%2F127.0.0.1%3A{port}%2Fannounce");
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let list_dir = root.join("list");
    let id = mgr.add(swiftget_engine::NewDownload { url: magnet, save_dir: Some(list_dir.clone()), start: true, ..Default::default() });
    assert_eq!(mgr.get(id).unwrap().file_name, "torrent");
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..600 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let v = mgr.get(id).unwrap();
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", v.error);
    assert_eq!(v.file_name, "song.bin", "the real name once the file list came");
    assert_eq!(std::fs::read(list_dir.join("song.bin")).unwrap(), data);
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A magnet link: its file list (a .torrent file) comes before the
/// download, from the people who share it, so files can be chosen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_link_file_list_first() {
    let Some((root, data, _file, seeder, sg, port)) = shared_file("meta").await else { return };
    let hash: String = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &make_info("song.bin", &data))
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http%3A%2F%2F127.0.0.1%3A{port}%2Fannounce");
    let getter = Torrents::new(&root.join("m"));
    let file = getter.metadata(&magnet, &root.join("listdir"), Duration::from_secs(60)).await.unwrap();
    let info = swiftget_engine::torrent::parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.name, "song.bin");
    assert_eq!(info.total, data.len() as u64);
    // nothing of the file itself was downloaded
    assert!(!root.join("listdir").join("song.bin").exists());
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// Complete with no sharing wanted: it is let go of at once, and the bits
/// of files that were not chosen go with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_sharing_lets_go_and_cleans_up() {
    let Some((root, _data, file, seeder, sg, _)) = shared_file("share").await else { return };
    let left = root.join("seed").join("left.bin");
    std::fs::write(&left, b"bits").unwrap();
    seeder.share_then_forget(&sg, 0, 0.0, vec![left.clone()]);
    let mut gone = false;
    for _ in 0..50 {
        if seeder.status(&sg).await.is_err() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "forgotten");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!left.exists(), "the leftover is deleted");
    assert!(root.join("seed").join("song.bin").exists(), "the file itself stays");
    // a later start of the same torrent is not ended by an older sharing
    let sg2 = seeder.add(file.to_str().unwrap(), &AddOptions { dir: root.join("seed"), ..Default::default() }).await.unwrap();
    seeder.share_then_forget(&sg2, 30, 0.0, Vec::new());
    let sg3 = seeder.add(file.to_str().unwrap(), &AddOptions { dir: root.join("seed"), first_last: true, ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg3, 30).await);
    seeder.forget(&sg3).await;
    let _ = std::fs::remove_dir_all(&root);
}
