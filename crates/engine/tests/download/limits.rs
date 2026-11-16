use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

async fn serve_limited(data: Arc<Vec<u8>>, refuse: Option<&'static [u8]>, delay_ms: u64, refused: Option<Arc<AtomicUsize>>) -> u16 {
    let open = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let (data, open, refused) = (data.clone(), open.clone(), refused.clone());
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
                let extra = open.fetch_add(1, Ordering::SeqCst) >= 4;
                if extra {
                    if let Some(resp) = refuse {
                        open.fetch_sub(1, Ordering::SeqCst);
                        if let Some(r) = &refused {
                            r.fetch_add(1, Ordering::SeqCst);
                        }
                        let _ = w.write_all(resp).await;
                        return;
                    }
                }
                let (a, b) = match range {
                    Some((a, b)) => (a, b.unwrap_or(data.len() - 1).min(data.len() - 1)),
                    None => (0, data.len() - 1),
                };
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len(),
                    b - a + 1
                );
                if w.write_all(head.as_bytes()).await.is_ok() {
                    for (n, chunk) in data[a..=b].chunks(32768).enumerate() {
                        // one too many: cut off after a little
                        if extra && n >= 2 {
                            break;
                        }
                        if w.write_all(chunk).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    port
}

async fn run_refused(tag: &str, host: &str, refuse: &'static [u8]) {
    let data: Arc<Vec<u8>> = Arc::new((0..6_000_000u32).map(|i| (i % 253) as u8).collect());
    let refused = Arc::new(AtomicUsize::new(0));
    let port = serve_limited(data.clone(), Some(refuse), 5, Some(refused.clone())).await;
    let root = std::env::temp_dir().join(format!("aknan-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let url = format!("http://{host}:{port}/big.bin");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: url.clone(),
        save_dir: Some(root.join("dl")),
        connections: Some(16),
        start: true,
        ..Default::default()
    });
    let mut errors = std::collections::HashSet::new();
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..3000 {
        let v = mgr.get(id).unwrap();
        status = v.status;
        if let Some(e) = v.error {
            errors.insert(e);
        }
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{errors:?}");
    assert!(errors.is_empty(), "no error was shown: {errors:?}");
    assert!(refused.load(Ordering::SeqCst) > 0, "the site did refuse some");
    let got = std::fs::read(mgr.get(id).unwrap().save_path).unwrap();
    assert!(got == *data);
    let learned = swiftget_engine::http_download::learned_limit(&url).unwrap();
    assert!(learned <= 8, "the site's next downloads start with fewer: {learned}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A site that takes only 4 connections at a time and answers "too many
/// requests" (HTTP 429) to the others: the download goes on with fewer
/// connections and finishes, without an error, and the site's next
/// downloads start with that many.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_site_that_allows_fewer_connections() {
    run_refused("429", "127.0.0.1", b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
}

/// A site that takes only 4 connections at a time and answers "forbidden"
/// (HTTP 403, as pixeldrain does) to the others: not an expired link. The
/// download goes on with fewer connections and finishes, without an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_site_that_forbids_extra_connections() {
    run_refused("403", "localhost", b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
}

/// A site that cuts off connections when more than 4 are open (it answers
/// them, then drops them after a little): the download goes on with fewer
/// connections and finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_site_that_drops_extra_connections() {
    let data: Arc<Vec<u8>> = Arc::new((0..8_000_000u32).map(|i| (i % 249) as u8).collect());
    let port = serve_limited(data.clone(), None, 4, None).await;
    let root = std::env::temp_dir().join(format!("aknan-drops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/f.bin"),
        save_dir: Some(root.join("dl")),
        connections: Some(16),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..4000 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    assert!(std::fs::read(mgr.get(id).unwrap().save_path).unwrap() == *data);
    let _ = std::fs::remove_dir_all(&root);
}
