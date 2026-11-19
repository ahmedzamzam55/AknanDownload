use super::build::apply_headers;
use super::cookies::{remembered_cookies, BrowserCookie};
use super::options::{quality_tag, with_tag, YtInfo, YtOption};
use super::sub_tracks::YtSubtitle;
use crate::http::{build_client, request, SendOrdered};
use crate::m3u8::{parse_master, parse_media, parse_subtitles_media, MAX_PLAYLIST_BYTES};
use crate::model::{fmt_size, RequestHeaders};

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

async fn fetch_m3u8(client: &reqwest::Client, url: &str, headers: &RequestHeaders, ua: &str, wait_secs: u64) -> anyhow::Result<(String, String)> {
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(wait_secs),
        request(client, url, headers, ua).send_ordered(),
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

async fn probe_seg_len(client: &reqwest::Client, seg_url: &str, headers: &RequestHeaders, ua: &str) -> Option<u64> {
    let mut rb = client.head(seg_url).header(reqwest::header::USER_AGENT, ua);
    if let Some(r) = &headers.referer {
        rb = rb.header(reqwest::header::REFERER, r);
    }
    for (k, v) in &headers.extra {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let resp = tokio::time::timeout(std::time::Duration::from_secs(4), rb.send_ordered()).await.ok()?.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
}

async fn fetch_hls_bundle(url: String, headers: RequestHeaders, ua: String) -> anyhow::Result<HlsFetched> {
    let client = build_client()?;
    let (text, final_url) = fetch_m3u8(&client, &url, &headers, &ua, 20).await?;
    if !text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U") {
        return Ok(HlsFetched { text, final_url, master_duration: 0, seg_len: 0 });
    }
    let (variants, separate_audio) = parse_master(&final_url, &text)?;
    let mut master_duration = 0u64;
    if let (Some(v), false) = (variants.first(), separate_audio) {
        if let Ok((v_text, v_url)) = fetch_m3u8(&client, &v.url, &headers, &ua, 8).await {
            if let Ok(pl) = parse_media(&v_url, &v_text) {
                master_duration = pl.duration().round() as u64;
            }
        }
    }
    let mut seg_len = 0u64;
    if variants.is_empty() {
        if let Ok(pl) = parse_media(&final_url, &text) {
            if let Some(part) = pl.parts.get(pl.parts.len() / 2) {
                seg_len = probe_seg_len(&client, &part.url, &headers, &ua).await.unwrap_or(0);
            }
        }
    }
    Ok(HlsFetched { text, final_url, master_duration, seg_len })
}

struct HlsFetched {
    text: String,
    final_url: String,
    master_duration: u64,
    seg_len: u64,
}

fn fill_headers(url: &str, cookies: Option<&[BrowserCookie]>, referer: Option<&str>, extra: &[(String, String)]) -> (RequestHeaders, String, Vec<(String, String)>) {
    let mut h = RequestHeaders { referer: referer.map(str::to_string), ..Default::default() };
    apply_headers(&mut h, extra);
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
    let ua = h.user_agent.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| crate::config::DEFAULT_USER_AGENT.to_string());
    let mut out_headers = vec![("User-Agent".to_string(), ua.clone())];
    if let Some(r) = &h.referer {
        if !r.is_empty() {
            out_headers.push(("Referer".into(), r.clone()));
        }
    }
    for (k, v) in &h.extra {
        out_headers.push((k.clone(), v.clone()));
    }
    (h, ua, out_headers)
}

/// Resolve an `.m3u8` / HLS URL directly with full browser headers (`Origin`, `Referer`, `Cookie`)
/// and probe its variant/segments so exact episode duration, byte size, and subtitles are returned.
pub(super) fn try_resolve_hls(
    url: &str,
    cookies: Option<&[BrowserCookie]>,
    referer: Option<&str>,
    extra_headers: &[(String, String)],
) -> anyhow::Result<Option<YtInfo>> {
    let (h, ua, out_headers) = fill_headers(url, cookies, referer, extra_headers);
    let fetched = run_blocking(fetch_hls_bundle(url.to_string(), h, ua))?;
    if !fetched.text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U") {
        return Ok(None);
    }
    let subtitles: Vec<YtSubtitle> = parse_subtitles_media(&fetched.final_url, &fetched.text)
        .into_iter()
        .map(|s| YtSubtitle { lang: s.lang, name: s.name, url: s.url, ext: "vtt".into(), auto: false, headers: out_headers.clone() })
        .collect();
    let (variants, separate_audio) = parse_master(&fetched.final_url, &fetched.text)?;
    if !variants.is_empty() {
        if separate_audio && variants.iter().all(|v| v.audio_url.is_none()) {
            return Ok(None);
        }
        let dur = fetched.master_duration;
        let options = variants
            .into_iter()
            .map(|v| {
                let q = if v.height > 0 { format!("{}p", v.height) } else if v.bandwidth > 0 { format!("{}k", v.bandwidth / 1000) } else { "Video".into() };
                let size = if v.bandwidth > 0 && dur > 0 { dur.saturating_mul(v.bandwidth) / 8 } else { 0 };
                let has_audio = v.audio_url.is_some();
                YtOption {
                    key: format!("{q}|TS"),
                    label: format!("{} · MP4", with_tag(&q, v.height)),
                    sub: if size > 0 { fmt_size(size) } else { String::new() },
                    quality: q,
                    video_url: v.url,
                    audio_url: v.audio_url,
                    ext: if has_audio { "mp4".into() } else { "ts".into() },
                    size,
                    headers: out_headers.clone(),
                    tag: quality_tag(v.height).into(),
                    codec: String::new(),
                    duration: dur,
                    kbps: v.bandwidth / 1000,
                }
            })
            .collect();
        return Ok(Some(YtInfo { title: String::new(), site: String::new(), options, subtitles }));
    }
    let pl = parse_media(&fetched.final_url, &fetched.text)?;
    if pl.parts.is_empty() {
        return Ok(None);
    }
    let dur = pl.duration().round() as u64;
    let size = if fetched.seg_len > 0 { fetched.seg_len.saturating_mul(pl.parts.len() as u64) } else { 0 };
    let kbps = if size > 0 && dur > 0 { (size * 8 / dur / 1000) as u64 } else { 0 };
    let options = vec![YtOption {
        key: "hls|TS".into(),
        label: "Video · MP4".into(),
        sub: if size > 0 { fmt_size(size) } else { String::new() },
        quality: "Video".into(),
        video_url: fetched.final_url,
        audio_url: None,
        ext: "ts".into(),
        size,
        headers: out_headers,
        tag: String::new(),
        codec: String::new(),
        duration: dur,
        kbps,
    }];
    Ok(Some(YtInfo { title: String::new(), site: String::new(), options, subtitles }))
}
