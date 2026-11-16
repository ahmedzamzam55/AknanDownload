use crate::helpers::{serve, serve_files, test_data};
use std::sync::Arc;
use std::time::Duration;

/// The folder changed in "Download File Info" while it was already
/// downloading (in the first folder), then stopped and resumed: the file in
/// the new folder must be the whole file, not a nearly empty one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_changed_while_downloading() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let root = std::env::temp_dir().join(format!("aknan-retarget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url, save_dir: Some(first.clone()), start: true, ..Default::default() });
    // part of it is there
    for _ in 0..400 {
        if mgr.get(id).map(|v| v.downloaded > 256 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    mgr.pause(id);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let name = mgr.get(id).unwrap().file_name;
    mgr.retarget(id, &second, &name, None, None).unwrap();
    mgr.start(id);
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(v.save_path.starts_with(&second), "{:?}", v.save_path);
    let got = std::fs::read(&v.save_path).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == *data, "the file in the new folder is not the downloaded file");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// The folder changed in "Download File Info" while it was already
/// downloading (in the first folder), then stopped and resumed: the file in
/// the new folder must be the whole file, not a nearly empty one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_changed_while_running() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let root = std::env::temp_dir().join(format!("aknan-retarget2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url, save_dir: Some(first.clone()), start: true, ..Default::default() });
    // part of it is there
    for _ in 0..400 {
        if mgr.get(id).map(|v| v.downloaded > 256 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let name = mgr.get(id).unwrap().file_name;
    mgr.retarget(id, &second, &name, None, None).unwrap();
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(v.save_path.starts_with(&second), "{:?}", v.save_path);
    let got = std::fs::read(&v.save_path).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == *data, "the file in the new folder is not the downloaded file");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// Picture and sound in two files (a video site): the folder changed while
/// it downloads. The joined file in the new folder must have both.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn video_and_sound_folder_changed() {
    let (Ok(v), Ok(a)) = (std::fs::read(std::env::var("AKNAN_TEST_VIDEO").unwrap_or_default()), std::fs::read(std::env::var("AKNAN_TEST_AUDIO").unwrap_or_default())) else {
        return; // test media not given
    };
    let (vlen, alen) = (v.len(), a.len());
    let port = serve_files(vec![("/v.mp4", Arc::new(v)), ("/a.m4a", Arc::new(a))]).await;
    let root = std::env::temp_dir().join(format!("aknan-retarget-av-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/v.mp4"),
        audio_url: Some(format!("http://127.0.0.1:{port}/a.m4a")),
        file_name: Some("clip.mp4".into()),
        save_dir: Some(first.clone()),
        early: true,
        start: true,
        ..Default::default()
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_ne!(mgr.get(id).unwrap().status, swiftget_engine::Status::Completed, "too fast for the test");
    mgr.pause(id);
    tokio::time::sleep(Duration::from_millis(300)).await;
    mgr.retarget(id, &second, "my clip.mp4", None, None).unwrap();
    mgr.start(id);
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let saved = mgr.get(id).unwrap().save_path;
    assert!(saved.starts_with(&second), "{saved:?}");
    let size = std::fs::metadata(&saved).unwrap().len() as usize;
    assert!(size > vlen && size > alen, "joined file is {size} bytes (video {vlen}, sound {alen})");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}
