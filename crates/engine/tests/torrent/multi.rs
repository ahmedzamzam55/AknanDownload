use crate::helpers::{ben, complete, make_multi, tracker};
use std::time::Duration;
use swiftget_engine::bt::Torrents;
use swiftget_engine::torrent::{parse_torrent, AddOptions};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_checked_files_of_a_torrent() {
    let root = std::env::temp_dir().join(format!("aknan-torrent-multi-{}", std::process::id()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("pack")).unwrap();
    let a: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let b: Vec<u8> = (0..90_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(seed.join("pack/a.bin"), &a).unwrap();
    std::fs::write(seed.join("pack/b.bin"), &b).unwrap();
    let port = tracker().await;
    let (t, _) = make_multi(&a, &b, &format!("http://127.0.0.1:{port}/announce"));
    let file = root.join("pack.torrent");
    std::fs::write(&file, t).unwrap();
    let info = parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.files.iter().map(|f| f.index).collect::<Vec<_>>(), [1, 2]);

    let seeder = Torrents::new(&root.join("a"));
    let sg = seeder.add(file.to_str().unwrap(), &AddOptions { dir: seed.clone(), ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await);
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let get = root.join("get");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(get.clone()),
        start: true,
        torrent_select: Some("2".into()),
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    let mut total = None;
    for _ in 0..600 {
        let v = mgr.get(id).unwrap();
        status = v.status;
        total = v.total_size;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    // its own size, not the pieces it shares with the other file
    assert_eq!(total, Some(b.len() as u64));
    assert_eq!(std::fs::read(get.join("pack/b.bin")).unwrap(), b);
    assert!(!get.join("pack/a.bin").exists(), "the unchecked file is not kept");
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A torrent whose file names Windows can't keep (a device name, characters
/// a name can't have) is written under safe names; the files can be changed
/// while it runs; its magnet link can be given.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsafe_names_files_changed_while_running_and_its_magnet() {
    let root = std::env::temp_dir().join(format!("aknan-torrent-safe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seed = root.join("seed");
    let a: Vec<u8> = (0..120_000u32).map(|i| (i % 249) as u8).collect();
    let b: Vec<u8> = (0..70_000u32).map(|i| (i % 239) as u8).collect();
    let port = tracker().await;
    // the same as make_multi, with names Windows refuses
    let piece = 16 * 1024;
    let all: Vec<u8> = a.iter().chain(&b).copied().collect();
    let mut pieces = Vec::new();
    for c in all.chunks(piece) {
        pieces.extend_from_slice(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, c).as_ref());
    }
    let file = |len: usize, name: &str| {
        let mut f = b"d6:length".to_vec();
        f.extend(format!("i{len}e").into_bytes());
        f.extend(b"4:pathl");
        f.extend(ben(name.as_bytes()));
        f.extend(b"ee");
        f
    };
    let mut info = b"d5:filesl".to_vec();
    info.extend(file(a.len(), "CON"));
    info.extend(file(b.len(), "a:b?.bin"));
    info.extend(b"e4:name");
    info.extend(ben(b"pack"));
    info.extend(b"12:piece lengthi16384e6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(format!("http://127.0.0.1:{port}/announce").as_bytes()));
    t.extend(b"4:info");
    t.extend(info);
    t.extend(b"e");
    let tfile = root.join("pack.torrent");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&tfile, &t).unwrap();
    let parsed = parse_torrent(&t).unwrap();
    let safe = swiftget_engine::torrent::safe_paths(&parsed);
    assert_eq!(safe.len(), 2, "{safe:?}");
    // the computer that shares it keeps the files under the safe names too
    for ((_, rel), data) in safe.iter().zip([&a, &b]) {
        let p = seed.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
    }
    let seeder = Torrents::new(&root.join("a"));
    let sg = seeder.add(tfile.to_str().unwrap(), &AddOptions { dir: seed.clone(), ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await, "the safe names are the ones it reads too");
    // slowly, so the change below comes while it runs
    seeder.set_limits(0, 40 * 1024).await;
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let get = root.join("get");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: tfile.to_string_lossy().into_owned(),
        save_dir: Some(get.clone()),
        start: true,
        torrent_select: Some("1".into()),
        ..Default::default()
    });
    // its magnet link: its id and name
    let m = mgr.magnet_link(id).unwrap();
    assert!(m.contains(&parsed.info_hash.clone().unwrap()) && m.contains("dn=pack"), "{m}");
    // while it runs: both files wanted after all
    tokio::time::sleep(Duration::from_millis(1500)).await;
    mgr.set_torrent_files(id, None);
    mgr.set_torrent_first_last(id, true);
    let mut v = mgr.get(id).unwrap();
    for _ in 0..900 {
        v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(v.status, swiftget_engine::Status::Completed, "{:?}", v.error);
    for ((_, rel), data) in safe.iter().zip([&a, &b]) {
        assert_eq!(&std::fs::read(get.join(rel)).unwrap(), data, "{rel}");
    }
    assert!(!get.join("pack/CON").exists() && !get.join("pack/a:b?.bin").exists());
    let live = mgr.torrent_live(id).unwrap();
    assert!(!live.pieces.is_empty());
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A torrent whose names lead out of its folder ("..") is refused, with words
/// that say why.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_way_out_of_the_folder_is_refused() {
    let root = std::env::temp_dir().join(format!("aknan-torrent-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let data = vec![7u8; 1000];
    let pieces = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data);
    let mut info = b"d5:filesld6:lengthi1000e4:pathl2:..8:evil.exeeee4:name4:pack12:piece lengthi16384e6:pieces".to_vec();
    info.extend(ben(pieces.as_ref()));
    info.extend(b"e");
    let mut t = b"d4:info".to_vec();
    t.extend(info);
    t.extend(b"e");
    let tfile = root.join("out.torrent");
    std::fs::write(&tfile, &t).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: tfile.to_string_lossy().into_owned(),
        save_dir: Some(root.join("get")),
        start: true,
        ..Default::default()
    });
    let mut v = mgr.get(id).unwrap();
    for _ in 0..100 {
        v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(v.status, swiftget_engine::Status::Failed);
    let e = v.error.unwrap_or_default();
    assert!(e.contains("lead out of its folder"), "{e}");
    assert!(!root.join("evil.exe").exists());
    let _ = std::fs::remove_dir_all(&root);
}
