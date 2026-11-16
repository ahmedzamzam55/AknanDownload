use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(crate) fn ffmpeg() -> Option<PathBuf> {
    let ok = std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false);
    ok.then(|| PathBuf::from("ffmpeg"))
}

/// A 2 minutes video (a whole picture every 2 s), its index at the end of the file.
pub(crate) fn make_video(dir: &Path) -> PathBuf {
    let out = dir.join("long.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=120"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=120:sample_rate=48000"])
        .args(["-c:v", "libx264", "-g", "50", "-b:v", "1000k", "-c:a", "aac", "-shortest"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(ok.success());
    out
}

/// A site that gives parts of one file (HTTP ranges) and counts what it sent.
pub(crate) async fn site(data: Arc<Vec<u8>>, sent: Arc<AtomicU64>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let (data, sent) = (data.clone(), sent.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let range = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string));
                let total = data.len() as u64;
                let (a, b) = match range {
                    Some(r) => {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let a: u64 = a.parse().unwrap();
                        let b: u64 = if b.is_empty() { total - 1 } else { b.parse::<u64>().unwrap().min(total - 1) };
                        (a, b)
                    }
                    None => (0, total - 1),
                };
                if std::env::var_os("SITE_LOG").is_some() { eprintln!("SITE range {a}-{b}"); }
                let body = &data[a as usize..=b as usize];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{total}\r\nContent-Length: {}\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if s.write_all(head.as_bytes()).await.is_ok() && s.write_all(body).await.is_ok() {
                    sent.fetch_add(body.len() as u64, Ordering::Relaxed);
                }
            });
        }
    });
    port
}

/// A site with the files of a folder (ranges too).
pub(crate) async fn folder_site(dir: PathBuf) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let dir = dir.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                let Ok(data) = std::fs::read(dir.join(&path)) else {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let total = data.len();
                let range = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string));
                let (a, b, code) = match range {
                    Some(r) => {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let b = if b.is_empty() { total - 1 } else { b.parse::<usize>().unwrap().min(total - 1) };
                        (a.parse::<usize>().unwrap(), b, "206 Partial Content")
                    }
                    None => (0, total - 1, "200 OK"),
                };
                let head = format!("HTTP/1.1 {code}\r\nContent-Range: bytes {a}-{b}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b - a + 1);
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&data[a..=b]).await;
            });
        }
    });
    port
}

pub(crate) async fn wait_done(mgr: &Arc<swiftget_engine::Manager>, id: u64) -> swiftget_engine::DownloadView {
    for _ in 0..9000 {
        let v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the part did not finish");
}
