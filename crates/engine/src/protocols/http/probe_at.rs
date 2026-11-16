use super::refresh::{is_bot_check, meta_refresh, refresh_target, set_cookies, PAGE_PEEK};
use super::request::{fallbacks, looks_like_hls, request, SendOrdered};
use super::{HttpStatus, NetDrop, ANSWER_TIMEOUT, CAPTCHA};
use crate::filename;
use crate::model::{DownloadKind, ProbeResult, RequestHeaders};
use anyhow::{bail, Context};
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::{Client, StatusCode};
use std::time::Duration;

/// Where a page leads (wait, address, cookies it set).
pub(super) type PageNext = (Duration, url::Url, Vec<String>);

pub(super) async fn probe_at(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
    follow_pages: bool,
) -> anyhow::Result<(ProbeResult, Option<reqwest::Response>, Option<PageNext>)> {
    let mut first_refusal: Option<u16> = None;
    let mut found = None;
    for (with_range, h) in fallbacks(headers) {
        let mut rb = request(client, url, &h, default_ua);
        if with_range {
            // from the start to the end, like a browser asking to resume:
            // some servers refuse a tiny range ("bytes=0-0") but take this.
            rb = rb.header(RANGE, "bytes=0-");
        }
        let resp = match tokio::time::timeout(ANSWER_TIMEOUT, rb.send_ordered()).await {
            Ok(r) => r.context("could not connect to server")?,
            Err(_) => return Err(NetDrop("the server did not answer (timed out)").into()),
        };
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            bail!("the server asks for a user name and password (HTTP 401) - use \"Use authorization\" or Options > Site Logins");
        }
        // the site's "not a robot" check: no other way of asking helps
        if is_bot_check(&resp) {
            return Err(anyhow::Error::new(HttpStatus(403)).context(CAPTCHA));
        }
        // refused (forbidden, range not allowed, too many requests...): try
        // the next way; anything else is the server's real answer
        if matches!(status.as_u16(), 403 | 406 | 416 | 429) {
            first_refusal.get_or_insert(status.as_u16());
            continue;
        }
        if !status.is_success() {
            return Err(HttpStatus(status.as_u16()).into());
        }
        // a form's answer that sent it on elsewhere: there the file is asked
        // for plainly (as the browser does after "303 See Other")
        let mut h = h;
        if h.post.is_some() && resp.url().as_str() != url {
            h.post = None;
            h.post_type = None;
        }
        let changed = h != *headers;
        found = Some((resp, with_range, changed.then_some(h)));
        break;
    }
    let Some((mut resp, with_range, headers_used)) = found else {
        return Err(HttpStatus(first_refusal.unwrap_or(403)).into());
    };
    let status = resp.status();
    let final_url = resp.url().to_string();
    let h = resp.headers().clone();
    let content_type = h.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let last_modified = h
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(s.trim()).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    // a strong ETag only ("W/..." may stay the same for another file)
    let etag = h
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with("W/"));

    let (total_size, resumable, from_zero) = if status == StatusCode::PARTIAL_CONTENT {
        // "bytes 0-123455/123456"
        let range = h.get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).unwrap_or("");
        let total = range.rsplit('/').next().and_then(|s| s.trim().parse::<u64>().ok());
        let start = range.trim().strip_prefix("bytes").and_then(|r| r.trim().split('-').next()).and_then(|s| s.trim().parse::<u64>().ok());
        (total, total.is_some(), start == Some(0))
    } else {
        // the whole file came, not a part: whatever "Accept-Ranges" says,
        // this server did not take a byte range, so one connection and no
        // resume (parts it would not honour were made again and again)
        (resp.content_length(), false, true)
    };

    let named = h.get(CONTENT_DISPOSITION).is_some();
    let name = h
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename::from_content_disposition)
        .or_else(|| filename::from_url(&final_url))
        .or_else(|| filename::from_url(url))
        .unwrap_or_else(|| "download".into());
    let kind = if looks_like_hls(&final_url, content_type.as_deref()) {
        DownloadKind::Hls
    } else {
        DownloadKind::Http
    };
    let mut file_name = filename::sanitize(&name);
    if kind == DownloadKind::Hls {
        // the stream gets joined into one .ts video file
        let (stem, ext) = filename::split_ext(&file_name);
        if ext.eq_ignore_ascii_case("m3u8") || ext.is_empty() {
            file_name = format!("{stem}.ts");
        }
    } else {
        file_name = filename::ensure_extension(file_name, content_type.as_deref());
    }
    // a server that refused a byte range gets one plain connection
    let resumable = resumable && with_range;
    let look = ProbeResult {
        headers_used,
        final_url,
        file_name,
        total_size: if kind == DownloadKind::Hls { None } else { total_size },
        resumable: kind == DownloadKind::Http && resumable,
        content_type,
        kind,
        last_modified,
        etag,
        digest: crate::checksum::Expected::from_headers(&h, status == StatusCode::OK).map(|e| e.text()),
    };
    // a small web page where a file was expected: a page that leads on to
    // the file by itself ("your download starts in a moment")?
    let small = resp.content_length().is_none_or(|n| n <= PAGE_PEEK as u64);
    {
        if follow_pages && look.is_web_page() && !named && small {
            let base = resp.url().clone();
            let refresh = h.get(reqwest::header::REFRESH).and_then(|v| v.to_str().ok()).and_then(|v| refresh_target(v, &base));
            let next = match refresh {
                Some(n) => Some(n),
                None => {
                    let mut page = Vec::new();
                    while page.len() < PAGE_PEEK {
                        match tokio::time::timeout(Duration::from_secs(10), resp.chunk()).await {
                            Ok(Ok(Some(c))) => page.extend_from_slice(&c),
                            _ => break,
                        }
                    }
                    meta_refresh(&String::from_utf8_lossy(&page), &base)
                }
            };
            if let Some((wait, next)) = next {
                return Ok((look, None, Some((wait, next, set_cookies(&h)))));
            }
            return Ok((look, None, None));
        }
    }
    // kept for the download: a file (not a page, not a stream list) whose
    // answer starts at its first byte
    let keepable = kind == DownloadKind::Http && from_zero && !look.is_web_page();
    Ok((look, keepable.then_some(resp), None))
}
