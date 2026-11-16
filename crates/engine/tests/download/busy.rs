use crate::helpers::test_data;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A busy server: after a third of the file every request gets "503" for a
/// while, and many answers break off half way. The download must finish by
/// itself (no "Resume" click), and the file must be right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_server_needs_no_resume_click() {
    let data = test_data(4 * 1024 * 1024 + 5);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sent = Arc::new(AtomicUsize::new(0));
    // when the busy time ends (0 = not busy yet)
    let busy_until = Arc::new(AtomicU64::new(0));
    let was_busy = Arc::new(AtomicBool::new(false));
    let served = Arc::new(AtomicUsize::new(0));
    {
        let (data, sent, busy_until, was_busy, served) = (data.clone(), sent.clone(), busy_until.clone(), was_busy.clone(), served.clone());
        tokio::spawn(async move {
            let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let (data, sent, busy_until, was_busy, served) = (data.clone(), sent.clone(), busy_until.clone(), was_busy.clone(), served.clone());
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut range: Option<(usize, usize)> = None;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = v.trim().split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                            range = Some((a, b.min(data.len() - 1)));
                        }
                    }
                    let until = busy_until.load(Ordering::SeqCst);
                    if until != 0 && now() < until {
                        let _ = w.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let n = served.fetch_add(1, Ordering::SeqCst);
                    let (a, b) = range.unwrap_or((0, data.len() - 1));
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"busy.bin\"\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    if w.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let body = &data[a..=b];
                    // every third answer breaks off half way
                    let cut = if n % 3 == 2 { body.len() / 2 } else { body.len() };
                    for chunk in body[..cut].chunks(8192) {
                        if busy_until.load(Ordering::SeqCst) > now() {
                            return; // busy now: the line drops
                        }
                        if w.write_all(chunk).await.is_err() {
                            return;
                        }
                        if sent.fetch_add(chunk.len(), Ordering::SeqCst) + chunk.len() > data.len() / 3 && !was_busy.swap(true, Ordering::SeqCst) {
                            busy_until.store(now() + 20_000, Ordering::SeqCst);
                        }
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                });
            }
        });
    }
    let root = std::env::temp_dir().join(format!("aknan-busy-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/busy.bin"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    let mut saw_failed = false;
    for _ in 0..6000 {
        status = mgr.get(id).unwrap().status;
        if status == swiftget_engine::Status::Failed {
            saw_failed = true;
        }
        if status == swiftget_engine::Status::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(was_busy.load(Ordering::SeqCst), "the server was busy for a while");
    assert!(!saw_failed, "it stopped and waited for a click: {:?}", mgr.get(id).unwrap().error);
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(std::fs::read(&v.save_path).unwrap() == *data, "the file is right");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}
