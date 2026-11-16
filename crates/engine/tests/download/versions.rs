use crate::helpers::{serve_files, test_data};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::http::{build_client, probe, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A server for the version tests: `version` picks the file (and its ETag);
/// `ranges` false = it ignores byte ranges (always the whole file, 200) but
/// still says "Accept-Ranges: bytes".
async fn serve_versions(files: Arc<Vec<Arc<Vec<u8>>>>, version: Arc<AtomicUsize>, ranges: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let (files, version) = (files.clone(), version.clone());
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, Option<usize>)> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        range = Some((a.parse().unwrap(), b.parse().ok()));
                    }
                }
                let v = version.load(Ordering::SeqCst);
                let data = files[v].clone();
                let (a, b, head) = match range.filter(|_| ranges) {
                    Some((a, b)) => {
                        let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
                        (a, b, format!(
                            "HTTP/1.1 206 Partial Content\r\nETag: \"v{v}\"\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"ver.bin\"\r\nConnection: close\r\n\r\n",
                            data.len(), b - a + 1))
                    }
                    None => (0, data.len() - 1, format!(
                        "HTTP/1.1 200 OK\r\nETag: \"v{v}\"\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"ver.bin\"\r\nConnection: close\r\n\r\n",
                        data.len())),
                };
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(8192) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
        }
    });
    port
}

/// "Accept-Ranges: bytes", but the range asked for is not honoured (the
/// whole file, HTTP 200): no resume, one connection, and the file is right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn range_ignored_means_one_connection() {
    let data = test_data(700_000);
    let files = Arc::new(vec![data.clone()]);
    let port = serve_versions(files, Arc::new(AtomicUsize::new(0)), false).await;
    let shared = Shared::new(build_client().unwrap(), SpeedLimiter::new(0));
    let p = probe(&shared.client(), &format!("http://127.0.0.1:{port}/ver.bin"), &RequestHeaders::default(), "test").await.unwrap();
    assert!(!p.resumable, "a range that was not honoured is no resume");
    assert_eq!(p.total_size, Some(data.len() as u64));
    let root = std::env::temp_dir().join(format!("aknan-norange-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url: format!("http://127.0.0.1:{port}/ver.bin"), save_dir: Some(root.join("out")), start: true, ..Default::default() });
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    assert!(std::fs::read(mgr.get(id).unwrap().save_path).unwrap() == *data);
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// The file changed on the server while the download was paused (the same
/// size, another ETag): it starts again, and the file is the new one only
/// (never half old, half new).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_changed_on_the_server_starts_again() {
    let old = test_data(2 * 1024 * 1024);
    let new: Arc<Vec<u8>> = Arc::new(old.iter().map(|b| b.wrapping_add(7)).collect());
    let files = Arc::new(vec![old.clone(), new.clone()]);
    let version = Arc::new(AtomicUsize::new(0));
    let port = serve_versions(files, version.clone(), true).await;
    let root = std::env::temp_dir().join(format!("aknan-etag-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out")).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url: format!("http://127.0.0.1:{port}/ver.bin"), save_dir: Some(root.join("out")), start: true, ..Default::default() });
    for _ in 0..500 {
        if mgr.get(id).map(|v| v.downloaded > 300 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    mgr.pause(id);
    for _ in 0..200 {
        if mgr.get(id).unwrap().status == swiftget_engine::Status::Paused {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(mgr.get(id).unwrap().downloaded > 0);
    version.store(1, Ordering::SeqCst);
    mgr.start(id);
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..2000 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let got = std::fs::read(mgr.get(id).unwrap().save_path).unwrap();
    assert!(got == *new, "the file is the new version only");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// A download that failed before it wrote anything is removed, and the same
/// file is added again at once: cleaning up the old one must not take the
/// new one's file away (it went on into nothing and was gone at 100 %).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_an_old_download_leaves_the_new_ones_file() {
    let data: Arc<Vec<u8>> = Arc::new((0..2_000_000u32).map(|i| (i % 251) as u8).collect());
    let port = serve_files(vec![("/big.bin", data.clone())]).await;
    let root = std::env::temp_dir().join(format!("aknan-same-name-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("dl");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    // the first try fails at once (nothing answers there)
    let old = mgr.add(swiftget_engine::NewDownload {
        url: "http://127.0.0.1:9/big.bin".into(),
        file_name: Some("big.bin".into()),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    for _ in 0..500 {
        if mgr.get(old).map(|v| v.status == swiftget_engine::Status::Failed).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    mgr.remove(old, false);
    let new = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/big.bin"),
        file_name: Some("big.bin".into()),
        save_dir: Some(dir.clone()),
        connections: Some(1),
        start: true,
        ..Default::default()
    });
    assert_eq!(mgr.get(new).unwrap().file_name, "big.bin");
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(new).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(new).unwrap().error);
    assert!(std::fs::read(dir.join("big.bin")).unwrap() == *data);
    let _ = std::fs::remove_dir_all(&root);
}
