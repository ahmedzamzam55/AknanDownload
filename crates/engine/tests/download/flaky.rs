use crate::helpers::test_data;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::http_download::{self, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Like `serve`, but while `down` is set every connection is dropped at once,
/// and the first answers break off half way: a flaky internet line.
async fn serve_flaky(data: Arc<Vec<u8>>, down: Arc<std::sync::atomic::AtomicBool>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(std::sync::atomic::AtomicU32::new(0));
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            if down.load(Ordering::Relaxed) {
                drop(sock);
                continue;
            }
            let data = data.clone();
            let n = served.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, usize)> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b.min(data.len() - 1)));
                    }
                }
                let (a, b) = range.unwrap_or((0, data.len() - 1));
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len(),
                    b - a + 1
                );
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let body = &data[a..=b];
                // the first requests lose the line after half of their part
                let cut = if n < 4 { body.len() / 2 } else { body.len() };
                for chunk in body[..cut].chunks(8192) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waits_through_a_long_outage_and_resumes() {
    let data = test_data(2 * 1024 * 1024 + 99);
    let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let port = serve_flaky(data.clone(), down.clone()).await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let dir = std::env::temp_dir().join(format!("swiftget-flaky-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let job = HttpJob {
        url,
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(data.len() as u64),
        resumable: true,
        part_path: part.clone(),
        connections: 4,
        min_split: 64 * 1024,
        // far fewer than the failures below: a dropped line must not use them up
        retries: 1,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check: Default::default(),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());

    // the line goes down once a bit is there, for longer than the old retries
    let (d2, p2) = (down.clone(), progress.clone());
    let watcher = tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < 200_000 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        d2.store(true, Ordering::Relaxed);
        let mut saw_waiting = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            saw_waiting |= p2.waiting.load(Ordering::Relaxed);
        }
        d2.store(false, Ordering::Relaxed);
        saw_waiting
    });
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new())
        .await
        .expect("should wait for the connection and finish");
    assert!(watcher.await.unwrap(), "should have shown 'waiting for connection'");
    assert!(!progress.waiting.load(Ordering::Relaxed));
    let out = std::fs::read(&part).unwrap();
    assert!(out == *data, "downloaded bytes differ from the source");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn network_errors_are_told_apart() {
    use swiftget_engine::http::{http_status_of, is_network_error, HttpStatus, NetDrop};
    let drop: anyhow::Error = NetDrop("connection closed early").into();
    assert!(is_network_error(&drop));
    assert!(is_network_error(&drop.context("part 3")));
    let reset: anyhow::Error = std::io::Error::from(std::io::ErrorKind::ConnectionReset).into();
    assert!(is_network_error(&reset));
    let full: anyhow::Error = std::io::Error::from(std::io::ErrorKind::StorageFull).into();
    assert!(!is_network_error(&full));
    let gone: anyhow::Error = HttpStatus(410).into();
    assert!(!is_network_error(&gone));
    assert_eq!(http_status_of(&gone.context("x")), Some(410));
}
