use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::http::{build_client_with, Progress, ProxyConfig, Shared};
use swiftget_engine::http_download::{self, FileCheck, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// What the test server was asked.
#[derive(Debug, Clone, Default)]
pub(crate) struct Asked {
    pub(crate) method: String,
    pub(crate) body: String,
    pub(crate) path: String,
    pub(crate) range: Option<(usize, Option<usize>)>,
    pub(crate) if_range: Option<String>,
    pub(crate) cookie: String,
}

/// What it answers: status line and headers (without length), body.
pub(crate) type Answer = (String, Vec<String>, Vec<u8>);

/// A server whose answers `handler` makes, sent slowly enough to pause in.
pub(crate) async fn serve(handler: Arc<dyn Fn(&Asked) -> Answer + Send + Sync>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let handler = handler.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut asked = Asked::default();
                let mut first = true;
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if first {
                        asked.method = line.split_whitespace().next().unwrap_or("").to_string();
                        asked.path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                        first = false;
                        continue;
                    }
                    let l = line.trim_end().to_string();
                    let lower = l.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("range: bytes=") {
                        let (a, b) = v.split_once('-').unwrap();
                        asked.range = Some((a.parse().unwrap(), b.parse().ok()));
                    } else if lower.starts_with("if-range:") {
                        asked.if_range = Some(l[9..].trim().to_string());
                    } else if lower.starts_with("cookie:") {
                        asked.cookie = l[7..].trim().to_string();
                    } else if let Some(n) = lower.strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap_or(0);
                    }
                }
                if length > 0 {
                    let mut body = vec![0u8; length];
                    let _ = tokio::io::AsyncReadExt::read_exact(&mut r, &mut body).await;
                    asked.body = String::from_utf8_lossy(&body).into_owned();
                }
                let (status, headers, body) = handler(&asked);
                let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                for h in headers {
                    head.push_str(&h);
                    head.push_str("\r\n");
                }
                head.push_str("\r\n");
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in body.chunks(16 * 1024) {
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

/// `data` the way a server with ranges sends it (`etag`: its version name).
pub(crate) fn ranged(data: &[u8], asked: &Asked, etag: Option<&str>) -> Answer {
    let mut headers = vec!["Accept-Ranges: bytes".to_string()];
    if let Some(e) = etag {
        headers.push(format!("ETag: {e}"));
    }
    // "If-Range" of another version: the whole (new) file
    let stale = matches!((&asked.if_range, etag), (Some(theirs), Some(ours)) if theirs != ours);
    match asked.range {
        Some((a, b)) if !stale && a < data.len() => {
            let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
            headers.push(format!("Content-Range: bytes {a}-{b}/{}", data.len()));
            ("206 Partial Content".into(), headers, data[a..=b].to_vec())
        }
        _ => ("200 OK".into(), headers, data.to_vec()),
    }
}

pub(crate) fn bytes(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 7) % 251) as u8).collect()
}

pub(crate) fn job(url: &str, size: usize, part: &std::path::Path, connections: u32, check: FileCheck) -> HttpJob {
    HttpJob {
        url: url.to_string(),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(size as u64),
        resumable: true,
        part_path: part.to_path_buf(),
        connections,
        min_split: 64 * 1024,
        retries: 2,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check,
    }
}

pub(crate) fn shared() -> Arc<Shared> {
    Arc::new(Shared::new(build_client_with(&ProxyConfig::default()).unwrap(), SpeedLimiter::new(0)))
}

pub(crate) fn temp(name: &str, port: u16) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aknan-same-{name}-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("f.part")
}

/// Run `j` until a quarter is there, then stop (a pause).
pub(crate) async fn run_a_quarter(j: &HttpJob, segs: &http_download::SegTable, size: usize) {
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let (c2, p2) = (cancel.clone(), progress.clone());
    tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < size as u64 / 4 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        c2.cancel();
    });
    assert!(http_download::run(shared(), j.clone(), segs.clone(), progress, cancel).await.is_err());
}

pub(crate) async fn wait_end(mgr: &Arc<swiftget_engine::Manager>, id: u64) -> swiftget_engine::DownloadView {
    for _ in 0..1500 {
        let v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("did not end");
}
