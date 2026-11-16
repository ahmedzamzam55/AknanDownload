use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

pub(crate) fn test_data(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| (i * 31 % 251) as u8).collect())
}

pub(crate) async fn serve(data: Arc<Vec<u8>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let data = data.clone();
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
                let (a, b, head) = match range {
                    Some((a, b)) => (a, b, format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"test file.bin\"\r\nConnection: close\r\n\r\n",
                        data.len(), b - a + 1)),
                    None => (0, data.len() - 1, format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())),
                };
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(8192) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    // the first part of the file is slow -> other connections split it
                    if a < data.len() / 8 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }
            });
        }
    });
    port
}

/// A web server with a few files by path (Range supported, a bit slow).
pub(crate) async fn serve_files(files: Vec<(&'static str, Arc<Vec<u8>>)>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let files = Arc::new(files);
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let files = files.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, Option<usize>)> = None;
                let mut path = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if path.is_empty() {
                        path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        range = Some((a.parse().unwrap(), b.parse().ok()));
                    }
                }
                let Some((_, data)) = files.iter().find(|(p, _)| *p == path) else {
                    let _ = w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let (a, b, head) = match range {
                    Some((a, b)) => {
                        let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
                        (a, b, format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len(), b - a + 1))
                    }
                    None => (0, data.len() - 1, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())),
                };
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(16384) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(15)).await;
                }
            });
        }
    });
    port
}
