use crate::model::RequestHeaders;
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE, COOKIE, REFERER, USER_AGENT};
use reqwest::{Client, RequestBuilder, StatusCode};

/// How many redirects one request follows.
const MAX_REDIRECTS: usize = 15;

/// Sending a request the way browsers and other download programs send it:
/// "Host" is the first line after the request line. The library puts it
/// last, and Cloudflare's protection refuses such a request (HTTP 403,
/// "challenge"), whatever else it carries (samfw.com and many more). The
/// client follows no redirects by itself, so every step of a redirect is
/// sent here too, with the "Host" of its own address.
pub trait SendOrdered {
    fn send_ordered(self) -> impl std::future::Future<Output = reqwest::Result<reqwest::Response>> + Send;
}

impl SendOrdered for RequestBuilder {
    async fn send_ordered(self) -> reqwest::Result<reqwest::Response> {
        let (client, req) = self.build_split();
        let mut req = req?;
        let mut steps = 0;
        loop {
            host_first(&mut req);
            let again = req.try_clone();
            let resp = client.execute(req).await?;
            let status = resp.status();
            if !status.is_redirection() || steps >= MAX_REDIRECTS {
                return Ok(resp);
            }
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|l| resp.url().join(l.trim()).ok())
                .filter(|u| matches!(u.scheme(), "http" | "https"));
            let (Some(next), Some(mut n)) = (next, again) else { return Ok(resp) };
            // 303, and 301 / 302 after a POST: the next step is a plain GET
            if status == StatusCode::SEE_OTHER
                || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND) && n.method() == reqwest::Method::POST)
            {
                *n.method_mut() = reqwest::Method::GET;
                *n.body_mut() = None;
                n.headers_mut().remove(CONTENT_TYPE);
                n.headers_mut().remove(reqwest::header::CONTENT_LENGTH);
            }
            // another site never gets this site's sign-in
            let prev = resp.url();
            // nor does a plain (http) address, where anyone on the way could read it
            let downgraded = prev.scheme() == "https" && next.scheme() != "https";
            if downgraded || next.host_str() != prev.host_str() || next.port_or_known_default() != prev.port_or_known_default() {
                for h in [reqwest::header::AUTHORIZATION, COOKIE, reqwest::header::PROXY_AUTHORIZATION, reqwest::header::WWW_AUTHENTICATE] {
                    n.headers_mut().remove(h);
                }
            }
            *n.url_mut() = next;
            req = n;
            steps += 1;
        }
    }
}

/// "Host" (with the port when it is not the usual one) as the first header.
fn host_first(req: &mut reqwest::Request) {
    let url = req.url();
    let Some(host) = url.host_str() else { return };
    let value = match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    };
    let Ok(value) = reqwest::header::HeaderValue::from_str(&value) else { return };
    let old = std::mem::take(req.headers_mut());
    let mut h = reqwest::header::HeaderMap::with_capacity(old.len() + 1);
    h.insert(reqwest::header::HOST, value);
    for (k, v) in old.iter() {
        if k != reqwest::header::HOST {
            h.append(k.clone(), v.clone());
        }
    }
    *req.headers_mut() = h;
}

/// GET request carrying the browser's headers.
pub fn request(client: &Client, url: &str, h: &RequestHeaders, default_ua: &str) -> RequestBuilder {
    let ua = h
        .user_agent
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default_ua);
    // a form's download is asked the way the form asks: POST with its data
    let form = h.post.as_deref().and_then(crate::checksum::base64);
    let rb = match form {
        Some(body) => client
            .post(url)
            .header(CONTENT_TYPE, h.post_type.as_deref().filter(|t| !t.is_empty()).unwrap_or("application/x-www-form-urlencoded"))
            .body(body),
        None => client.get(url),
    };
    // identity: ranges must refer to the raw bytes, never a compressed body
    let mut rb = rb
        .header(USER_AGENT, ua)
        .header(ACCEPT, "*/*")
        .header(ACCEPT_ENCODING, "identity");
    if let Some(r) = h.referer.as_deref().filter(|s| !s.is_empty()) {
        rb = rb.header(REFERER, r);
    }
    if let Some(c) = h.cookies.as_deref().filter(|s| !s.is_empty()) {
        rb = rb.header(COOKIE, c);
    }
    for (k, v) in &h.extra {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if let Some((user, pass)) = h.auth.as_ref().filter(|(u, _)| !u.is_empty()) {
        rb = rb.basic_auth(user, Some(pass));
    }
    rb
}

pub(crate) fn looks_like_hls(url: &str, content_type: Option<&str>) -> bool {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("mpegurl") {
        return true;
    }
    url::Url::parse(url)
        .map(|u| {
            let p = u.path().to_ascii_lowercase();
            p.ends_with(".m3u8") || p.ends_with(".m3u") || (p.contains("/hls") && p.ends_with(".txt"))
        })
        .unwrap_or(false)
}

/// Ways to ask again when a server refuses the request as the browser sent
/// it: some sites refuse a byte range, or cookies / a referrer that belong to
/// the browser's own session. (with range?, headers)
pub(super) fn fallbacks(h: &RequestHeaders) -> Vec<(bool, RequestHeaders)> {
    let mut out = vec![(true, h.clone()), (false, h.clone())];
    let has = |s: &Option<String>| s.as_deref().map(|v| !v.is_empty()).unwrap_or(false);
    if has(&h.cookies) {
        let mut n = h.clone();
        n.cookies = None;
        out.push((true, n.clone()));
        out.push((false, n));
    }
    if has(&h.cookies) || has(&h.referer) || has(&h.user_agent) {
        let mut n = h.clone();
        n.cookies = None;
        n.referer = None;
        n.user_agent = None;
        out.push((true, n.clone()));
        out.push((false, n));
    }
    out
}
