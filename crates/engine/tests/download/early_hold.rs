use crate::helpers::serve;
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{NewDownload, Status};

/// Option B Safe Hold: while "Download File Info" is open (`hold_before_finish`),
/// even if all bytes finish arriving, nothing is moved into `save_dir` and any
/// existing file stays intact until `confirm_early` is called.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn early_hold_stays_in_sgpart_until_confirmed() {
    let data = Arc::new(vec![42u8; 256 * 1024]);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/clip.mp4");
    let root = std::env::temp_dir().join(format!("aknan-hold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let existing = out.join("clip.mp4");
    std::fs::write(&existing, b"original-old-file").unwrap();

    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(NewDownload {
        url,
        file_name: Some("clip.mp4".into()),
        save_dir: Some(out.clone()),
        early: true,
        hold_before_finish: true,
        start: true,
        ..Default::default()
    });

    // Give it plenty of time to download all 256 KB
    tokio::time::sleep(Duration::from_millis(500)).await;
    let v = mgr.get(id).unwrap();
    assert_ne!(v.status, Status::Completed, "must NOT complete before Start is pressed");
    assert_eq!(std::fs::read(&existing).unwrap(), b"original-old-file", "old file must stay intact");

    // Confirm with safe replace
    mgr.confirm_early(id, &out, "clip.mp4", None, None, None, true).unwrap();
    let mut status = Status::Queued;
    for _ in 0..500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, Status::Completed | Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    assert_eq!(std::fs::read(&existing).unwrap(), *data);
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// Clicking "Download Later" while a safe-held early download finished its
/// `.sgpart` must pause cleanly without putting the file into `save_dir`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn early_hold_download_later_keeps_save_dir_clean() {
    let data = Arc::new(vec![9u8; 128 * 1024]);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/later.bin");
    let root = std::env::temp_dir().join(format!("aknan-hold-later-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let out = root.join("out");
    std::fs::create_dir_all(&out).unwrap();

    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(NewDownload {
        url,
        file_name: Some("later.bin".into()),
        save_dir: Some(out.clone()),
        early: true,
        hold_before_finish: true,
        start: true,
        ..Default::default()
    });
    tokio::time::sleep(Duration::from_millis(400)).await;
    mgr.pause_early(id, &out, "later.bin", None, None, Some(1), None, false).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(mgr.get(id).unwrap().status, Status::Paused);
    assert!(!out.join("later.bin").exists(), "Download Later must NOT place file in folder");
    mgr.start(id);
    let mut status = Status::Paused;
    for _ in 0..250 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, Status::Completed | Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    assert_eq!(std::fs::read(out.join("later.bin")).unwrap(), *data);
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}
