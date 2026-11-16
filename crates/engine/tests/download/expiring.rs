use crate::helpers::test_data;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A link that expires: "/get" sends the browser on to "/file?t=N"; after
/// about a third of the file every old token answers 403. The download asks
/// "/get" again, checks that the new link is the same file and goes on.
async fn serve_expiring(data: Arc<Vec<u8>>, wrong_size_once: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = Arc::new(AtomicU64::new(1));
    let sent = Arc::new(AtomicUsize::new(0));
    let wrong = Arc::new(AtomicBool::new(wrong_size_once));
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let (data, token, sent, wrong) = (data.clone(), token.clone(), sent.clone(), wrong.clone());
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let (mut path, mut range) = (String::new(), None::<(usize, usize)>);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if path.is_empty() {
                        path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b.min(data.len() - 1)));
                    }
                }
                if path == "/get" {
                    // a fresh link each time it is asked
                    let t = token.load(Ordering::SeqCst);
                    let head = format!("HTTP/1.1 302 Found\r\nLocation: /file?t={t}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = w.write_all(head.as_bytes()).await;
                    return;
                }
                let t: u64 = path.rsplit('=').next().and_then(|x| x.parse().ok()).unwrap_or(0);
                if t != token.load(Ordering::SeqCst) {
                    let _ = w.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
                // the first new link once gives another (smaller) file
                let len = if t == 2 && wrong.swap(false, Ordering::SeqCst) { data.len() / 2 } else { data.len() };
                let (a, b) = range.unwrap_or((0, len - 1));
                let b = b.min(len - 1);
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{len}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"exp.bin\"\r\nConnection: close\r\n\r\n",
                    b - a + 1
                );
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(8192) {
                    if t != token.load(Ordering::SeqCst) {
                        return; // expired while sending: the connection drops
                    }
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    // expire once, after a third of the file
                    if sent.fetch_add(chunk.len(), Ordering::SeqCst) + chunk.len() > data.len() / 3 && token.load(Ordering::SeqCst) == 1 {
                        token.store(2, Ordering::SeqCst);
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
        }
    });
    port
}

async fn run_expiring(wrong_size_once: bool) {
    let data = test_data(3 * 1024 * 1024 + 11);
    let port = serve_expiring(data.clone(), wrong_size_once).await;
    let root = std::env::temp_dir().join(format!("aknan-expire-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/get"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..3000 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(std::fs::read(&v.save_path).unwrap() == *data, "the file is right after the new link");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_link_is_renewed() {
    run_expiring(false).await;
}

/// The first new link gives another file: nothing of it is written, the
/// next one (the right file) is used.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_new_link_is_not_used() {
    run_expiring(true).await;
}
