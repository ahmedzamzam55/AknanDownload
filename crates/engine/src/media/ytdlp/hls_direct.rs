use super::build::apply_headers;
use super::cookies::{remembered_cookies, BrowserCookie};
use super::options::{quality_tag, with_tag, YtInfo, YtOption};
use crate::http::{build_client, request, SendOrdered};
use crate::m3u8::{parse_master, parse_media, MAX_PLAYLIST_BYTES};
use crate::model::RequestHeaders;

fn cookie_header_for(url: &str, list: &[BrowserCookie]) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let https = u.scheme() == "https";
    let path = u.path();
    let mut out = Vec::new();
    for c in list {
        let dom = c.domain.trim_start_matches('.').to_ascii_lowercase();
        let host_ok = host == dom || host.ends_with(&format!(".{dom}"));
        let path_ok = c.path.is_empty() || path.starts_with(&c.path);
        if host_ok && path_ok && (!c.secure || https) && !c.name.is_empty() {
            out.push(format!("{}={}", c.name, c.value));
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

fn run_blocking<F, T>(fut: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::spawn(|| {
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(fut)
        })
        .join()
        .map_err(|_| anyhow::anyhow!("worker panicked"))?;
    }
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(fut)
}

async fn fetch_m3u8(url: String, headers: RequestHeaders, ua: String) -> anyhow::Result<(String, String)> {
    let client = build_client()?;
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        request(&client, &url, &headers, &ua).send_ordered(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out"))??;
    if !resp.status().is_success() {
        return Err(crate::http::HttpStatus(resp.status().as_u16()).into());
    }
    let final_url = resp.url().to_string();
    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_PLAYLIST_BYTES {
        anyhow::bail!("playlist is too big");
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url))
}

/// Resolve an `.m3u8` / HLS URL directly with full browser headers (`Origin`, `Referer`, `Cookie`)
/// so protected embedded video servers never fail with `HTTP 403`.
pub(super) fn try_resolve_hls(
    url: &str,
    cookies: Option<&[BrowserCookie]>,
    referer: Option<&str>,
    extra_headers: &[(String, String)],
) -> anyhow::Result<Option<YtInfo>> {
    let mut h = RequestHeaders { referer: referer.map(str::to_string), ..Default::default() };
    apply_headers(&mut h, extra_headers);
    if h.cookies.is_none() {
        let kept = cookies
            .filter(|c| !c.is_empty())
            .map(<[BrowserCookie]>::to_vec)
            .or_else(|| remembered_cookies(url))
            .or_else(|| referer.and_then(remembered_cookies));
        if let Some(list) = kept {
            h.cookies = cookie_header_for(url, &list);
        }
    }
    let ua = h
        .user_agent
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::config::DEFAULT_USER_AGENT.to_string());
    let (text, final_url) = run_blocking(fetch_m3u8(url.to_string(), h.clone(), ua.clone()))?;
    if !text.trim_start().starts_with("#EXTM3U") {
        return Ok(None);
    }
    let mut out_headers = vec![("User-Agent".to_string(), ua)];
    if let Some(r) = &h.referer {
        if !r.is_empty() {
            out_headers.push(("Referer".into(), r.clone()));
        }
    }
    for (k, v) in &h.extra {
        out_headers.push((k.clone(), v.clone()));
    }
    let (variants, separate_audio) = parse_master(&final_url, &text)?;
    if !variants.is_empty() {
        if separate_audio {
            return Ok(None);
        }
        let options = variants
            .into_iter()
            .map(|v| {
                let q = if v.height > 0 {
                    format!("{}p", v.height)
                } else if v.bandwidth > 0 {
                    format!("{}k", v.bandwidth / 1000)
                } else {
                    "Video".into()
                };
                YtOption {
                    key: format!("{q}|TS"),
                    label: format!("{} · MP4", with_tag(&q, v.height)),
                    sub: String::new(),
                    quality: q,
                    video_url: v.url,
                    audio_url: None,
                    ext: "ts".into(),
                    size: 0,
                    headers: out_headers.clone(),
                    tag: quality_tag(v.height).into(),
                    codec: String::new(),
                    duration: 0,
                    kbps: v.bandwidth / 1000,
                }
            })
            .collect();
        return Ok(Some(YtInfo { title: String::new(), site: String::new(), options }));
    }
    let pl = parse_media(&final_url, &text)?;
    if pl.parts.is_empty() {
        return Ok(None);
    }
    let options = vec![YtOption {
        key: "hls|TS".into(),
        label: "Video · MP4".into(),
        sub: String::new(),
        quality: "Video".into(),
        video_url: final_url,
        audio_url: None,
        ext: "ts".into(),
        size: 0,
        headers: out_headers,
        tag: String::new(),
        codec: String::new(),
        duration: pl.duration().round() as u64,
        kbps: 0,
    }];
    Ok(Some(YtInfo { title: String::new(), site: String::new(), options }))
}
