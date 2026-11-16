use crate::helpers::test_data;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::http_download::{self, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// The last part goes to a connection that brings almost nothing (the server
/// keeps it alive, so it never times out): the download must not stand at
/// 90-99 % for ever. The trickle happens once; a new connection is fast.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_last_connection_does_not_hold_the_end() {
    let data = test_data(3 * 1024 * 1024);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let trickled = Arc::new(AtomicBool::new(false));
    {
        let data = data.clone();
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let (data, trickled) = (data.clone(), trickled.clone());
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut range = (0usize, data.len() - 1);
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = v.trim().split_once('-').unwrap();
                            let b = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                            range = (a.parse().unwrap(), b.min(data.len() - 1));
                        }
                    }
                    let (a, b) = range;
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    if w.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    // the first request for the last part: a trickle
                    let slow = a >= data.len() * 3 / 4 && !trickled.swap(true, Ordering::Relaxed);
                    for chunk in data[a..=b].chunks(if slow { 512 } else { 16384 }) {
                        if w.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(if slow { 1000 } else { 2 })).await;
                    }
                });
            }
        });
    }
    let url = format!("http://127.0.0.1:{port}/slow-end.bin");
    let dir = std::env::temp_dir().join(format!("swiftget-slowend-{port}"));
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
        min_split: 256 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check: Default::default(),
    };
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let started = std::time::Instant::now();
    let (segs, progress) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Progress::default()));
    if std::env::var_os("SLOWEND_DEBUG").is_some() {
        let (segs, progress) = (segs.clone(), progress.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(4)).await;
                let t: Vec<String> = segs.lock().unwrap().iter().map(|s: &swiftget_engine::segments::Segment| format!("{}..{}..{}{}", s.start, s.pos, s.end, if s.active { "*" } else { "" })).collect();
                eprintln!("{} conns={} {:?}", progress.bytes.load(Ordering::Relaxed), progress.connections.load(Ordering::Relaxed), t);
            }
        });
    }
    let run = http_download::run(shared, job, segs, progress, CancellationToken::new());
    tokio::time::timeout(Duration::from_secs(60), run).await.expect("held at the end").unwrap();
    assert!(started.elapsed() < Duration::from_secs(45), "took {:?}", started.elapsed());
    assert!(std::fs::read(&part).unwrap() == *data);
    let _ = std::fs::remove_dir_all(&dir);
}
