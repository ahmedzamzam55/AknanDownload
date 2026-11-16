use crate::helpers::test_data;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A server that takes some requests and never answers them (no headers, or
/// it stops sending): YouTube does this now and then. The download must not
/// sit at 0 waiting for ever: it asks again by itself and finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn silent_server_does_not_hold_the_download() {
    let data = test_data(3 * 1024 * 1024 + 3);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(AtomicUsize::new(0));
    {
        let (data, asked) = (data.clone(), asked.clone());
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let (data, asked) = (data.clone(), asked.clone());
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
                    let n = asked.fetch_add(1, Ordering::SeqCst);
                    // the very first request (the size check) and every fourth
                    // one: the connection stays open, nothing is ever sent
                    if n == 0 || n % 4 == 3 {
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                        drop(w);
                        return;
                    }
                    let (a, b) = range.unwrap_or((0, data.len() - 1));
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"quiet.bin\"\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    if w.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let body = &data[a..=b];
                    // every fifth answer stops half way and then keeps quiet
                    let quiet_after = if n % 5 == 4 { body.len() / 2 } else { body.len() };
                    for chunk in body[..quiet_after].chunks(8192) {
                        if w.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    if quiet_after < body.len() {
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                    }
                });
            }
        });
    }
    let root = std::env::temp_dir().join(format!("aknan-quiet-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let mut s = mgr.settings();
    s.connections = 8;
    mgr.set_settings(s).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/quiet.bin"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    let mut saw_failed = false;
    // a little over two minutes at most
    for _ in 0..7000 {
        status = mgr.get(id).unwrap().status;
        saw_failed |= status == swiftget_engine::Status::Failed;
        if status == swiftget_engine::Status::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!saw_failed, "it stopped: {:?}", mgr.get(id).unwrap().error);
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(std::fs::read(&v.save_path).unwrap() == *data, "the file is right");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}
