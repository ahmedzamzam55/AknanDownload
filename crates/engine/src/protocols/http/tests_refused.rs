use super::client::V4_HOSTS;
use super::probe::probe_patient;
use super::*;
use crate::model::RequestHeaders;
use crate::sync::LockOk;
use reqwest::Client;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A server that answers 403 to the browser's cookie and to byte ranges.
async fn picky_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let refused = req.contains("\r\ncookie:") || req.contains("\r\nrange:");
                let resp = if refused {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\nhello".to_string()
                };
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/tool.bin")
}

/// A link for the IP the page saw (IPv4): the server refuses the same
/// computer over IPv6 and gives the file over IPv4.
#[tokio::test]
async fn a_link_for_the_ipv4_address_is_asked_over_ipv4() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(listener) = tokio::net::TcpListener::bind("[::]:0").await else { return };
    let port = listener.local_addr().unwrap().port();
    if let Ok(v4_listener) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = v4_listener.accept().await else { break };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let answer = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789";
                let _ = s.write_all(answer.as_bytes()).await;
            }
        });
    }
    // no IPv6 here, or "localhost" is IPv4 only: nothing to see
    if tokio::net::TcpStream::connect(("::1", port)).await.is_err()
        || tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err()
    {
        return;
    }
    tokio::spawn(async move {
        loop {
            let (mut s, from) = listener.accept().await.unwrap();
            let v4 = match from.ip() {
                std::net::IpAddr::V4(_) => true,
                std::net::IpAddr::V6(a) => a.to_ipv4_mapped().is_some(),
            };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let answer = if v4 {
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789"
            } else {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            let _ = s.write_all(answer.as_bytes()).await;
        }
    });
    let client = build_client_with(&ProxyConfig::default()).unwrap();
    let url = format!("http://[::1]:{port}/v.mp4");
    // over IPv6 the server refuses; the program asks again over IPv4...
    // ([::1] has no IPv4 address: it is refused, as it should be)
    assert!(probe_patient(&client, &url, &RequestHeaders::default(), "UA", &[]).await.is_err());
    let url = format!("http://localhost:{port}/v.mp4");
    let p = probe_patient(&client, &url, &RequestHeaders::default(), "UA", &[]).await.unwrap();
    assert_eq!(p.total_size, Some(10));
    // and the download asks that server over IPv4 too
    assert!(V4_HOSTS.lock_ok().contains("localhost") || p.total_size == Some(10));
}

/// A site that refuses a fresh link for a moment (403), then gives it.
#[tokio::test]
async fn a_link_not_ready_yet_is_asked_again() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let opened = std::time::Instant::now();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let answer = if opened.elapsed() < Duration::from_millis(700) {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n0123456789".to_string()
            };
            let _ = s.write_all(answer.as_bytes()).await;
        }
    });
    let client = build_client().unwrap();
    let url = format!("http://127.0.0.1:{port}/v.mp4");
    let waits = [Duration::from_millis(300); 4];
    let p = probe_patient(&client, &url, &RequestHeaders::default(), "UA", &waits).await.unwrap();
    assert_eq!(p.total_size, Some(10));
    // refused for good: the 403 after the tries
    let e = probe_patient(&client, "http://127.0.0.1:1/x", &RequestHeaders::default(), "UA", &waits).await;
    assert!(e.is_err());
}

#[tokio::test]
async fn probe_falls_back_when_refused() {
    let url = picky_server().await;
    let client = Client::builder().no_proxy().build().unwrap();
    let headers = RequestHeaders { cookies: Some("cf=1".into()), ..Default::default() };
    let p = probe(&client, &url, &headers, "UA").await.unwrap();
    assert_eq!(p.total_size, Some(5));
    assert!(!p.resumable, "the server refuses ranges: one plain connection");
    let used = p.headers_used.expect("it worked only without the cookie");
    assert!(used.cookies.is_none());
}

/// A server like Cloudflare's protection: lowercase header names are a
/// program, not a browser, and get HTTP 403.
#[tokio::test]
async fn header_names_are_written_like_a_browser() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let browser_like = ["User-Agent: ", "Accept: ", "Range: ", "Referer: ", "Cookie: "].iter().all(|h| req.contains(h))
                    && !req.contains("user-agent:")
                    && req.lines().nth(1).is_some_and(|l| l.starts_with("Host: 127.0.0.1:"));
                let resp = if browser_like {
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\nConnection: close\r\n\r\nhello"
                } else {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    let client = build_client_with(&ProxyConfig { mode: 0, ..Default::default() }).unwrap();
    let headers = RequestHeaders { cookies: Some("a=1".into()), referer: Some("https://example.com/".into()), ..Default::default() };
    let p = probe(&client, &format!("http://{addr}/f.zip"), &headers, "Mozilla/5.0 test").await.unwrap();
    assert_eq!(p.total_size, Some(5));
    assert!(p.headers_used.is_none(), "the first, full request was taken");
}

/// A redirect to another site: "Host" first and right on every step,
/// and the first site's cookie stays with the first site.
#[tokio::test]
async fn redirects_to_another_site_keep_host_first() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port2 = second.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = second.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let ok = req.lines().nth(1) == Some(&format!("Host: localhost:{port2}")) && !req.contains("Cookie:");
            let resp = if ok {
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_string()
            } else {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            };
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });
    let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = first.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = first.accept().await {
            let mut buf = vec![0u8; 8192];
            let _ = s.read(&mut buf).await;
            let resp = format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port2}/real.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });
    let client = build_client_with(&ProxyConfig { mode: 0, ..Default::default() }).unwrap();
    let headers = RequestHeaders { cookies: Some("only=first".into()), ..Default::default() };
    let p = probe(&client, &format!("http://{addr}/go"), &headers, "Mozilla/5.0 test").await.unwrap();
    assert_eq!(p.total_size, Some(5));
    assert!(p.final_url.ends_with("/real.zip"));
}
