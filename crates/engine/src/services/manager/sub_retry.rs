use crate::http::{self, SendOrdered};
use crate::http_download::Cancelled;
use crate::model::RequestHeaders;
use crate::ytdlp;
use anyhow::{anyhow, bail};
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Decode standard or URL-safe base64 without adding external crates.
fn decode_b64(input: &str) -> Option<Vec<u8>> {
    let mut acc = 0u32;
    let mut bits = 0u8;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    for b in input.bytes() {
        if b.is_ascii_whitespace() || b == b'=' {
            continue;
        }
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Extract text payload from `data:` URIs (either `;base64,` or percent-encoded).
pub(super) fn decode_data_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("data:")?;
    let (meta, body) = rest.split_once(',')?;
    if meta.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
        let bytes = decode_b64(body)?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        Some(percent_encoding::percent_decode_str(body).decode_utf8_lossy().into_owned())
    }
}

pub(super) fn parse_retry_after(val: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    let s = val?.to_str().ok()?.trim();
    if let Ok(secs) = s.parse::<u64>() {
        return Some(Duration::from_secs(secs.clamp(1, 30)));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        let diff = dt.signed_duration_since(chrono::Utc::now()).num_seconds().max(1) as u64;
        return Some(Duration::from_secs(diff.min(30)));
    }
    None
}

/// Fetch subtitle text with cancel-aware `429`/`503`/`502`/`504` retries honoring `Retry-After`.
pub(super) async fn fetch_sub_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: &RequestHeaders,
    ua: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<String> {
    if url.starts_with("data:") {
        return decode_data_url(url).ok_or_else(|| anyhow!("invalid data: subtitle URL"));
    }
    for attempt in 0..5u32 {
        let req = tokio::time::timeout(Duration::from_secs(20), http::request(client, url, headers, ua).send_ordered());
        let resp = tokio::select! {
            res = req => match res {
                Ok(Ok(r)) => r,
                Ok(Err(e)) if attempt < 4 => {
                    wait_cancel(Duration::from_secs(1 << attempt.min(3)), cancel).await?;
                    let _ = e;
                    continue;
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) if attempt < 4 => {
                    wait_cancel(Duration::from_secs(1 << attempt.min(3)), cancel).await?;
                    continue;
                }
                Err(_) => bail!("subtitle download timed out"),
            },
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        let code = resp.status().as_u16();
        if matches!(code, 429 | 502 | 503 | 504) && attempt < 4 {
            let wait = parse_retry_after(resp.headers().get(reqwest::header::RETRY_AFTER))
                .unwrap_or_else(|| Duration::from_secs(2u64 << attempt.min(3)));
            wait_cancel(wait, cancel).await?;
            continue;
        }
        if !resp.status().is_success() {
            bail!("HTTP {code}");
        }
        let bytes = tokio::select! {
            b = resp.bytes() => b?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    bail!("subtitle server busy (HTTP 429/503)")
}

async fn wait_cancel(dur: Duration, cancel: &CancellationToken) -> anyhow::Result<()> {
    tokio::select! {
        _ = tokio::time::sleep(dur) => Ok(()),
        _ = cancel.cancelled() => Err(Cancelled.into()),
    }
}

/// Retry `ytdlp::fetch_subtitles` up to 3 times on transient server throttling.
pub(super) async fn fetch_ytdlp_sub_retry(
    page: String,
    dir: &Path,
    stem: String,
    langs: String,
    cancel: Option<&CancellationToken>,
) -> anyhow::Result<()> {
    let dir = dir.to_path_buf();
    for attempt in 0..3u32 {
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err(Cancelled.into());
        }
        let (p, d, s, l) = (page.clone(), dir.clone(), stem.clone(), langs.clone());
        let res = tokio::task::spawn_blocking(move || ytdlp::fetch_subtitles(&p, &d, &s, &l)).await?;
        if res.is_ok() || attempt == 2 {
            return res;
        }
        let wait = Duration::from_secs(2 + u64::from(attempt) * 3);
        match cancel {
            Some(c) => wait_cancel(wait, c).await?,
            None => tokio::time::sleep(wait).await,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn decodes_data_urls() {
        let b64 = decode_data_url("data:text/vtt;charset=utf-8;base64,V0VCVlRUCgowMDowMS4wMDAgLS0+IDAwOjAyLjAwMApIaQo=").unwrap();
        assert!(b64.starts_with("WEBVTT") && b64.contains("Hi"));
        let pct = decode_data_url("data:text/plain,WEBVTT%0A%0A00%3A01.000%20--%3E%2000%3A02.000%0AYo").unwrap();
        assert!(pct.contains("Yo"));
    }

    #[tokio::test]
    async fn retries_429_with_retry_after() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let resp = if attempt == 0 {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nConnection: close\r\n\r\nwait"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/vtt\r\nConnection: close\r\n\r\nWEBVTT\n\n00:00:01.000 --> 00:00:02.000\nOK\n"
                };
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        let client = reqwest::Client::new();
        let cancel = CancellationToken::new();
        let txt = fetch_sub_with_retry(&client, &format!("http://{addr}/sub.vtt"), &RequestHeaders::default(), "UA", &cancel)
            .await
            .unwrap();
        let _ = srv.await;
        assert!(txt.contains("OK"));
    }
}
