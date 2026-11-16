use super::client::{client_for, host_of, v4_client, V4_HOSTS};
use super::probe_at::probe_at;
use super::refresh::{add_cookies, keep, kept_look, MAX_PAGE_HOPS};
use super::{http_status_of, wants_captcha};
use crate::model::{ProbeResult, RequestHeaders};
use crate::sync::LockOk;
use anyhow::bail;
use reqwest::Client;
use std::time::Duration;

/// Ask the server about a file: real size, final URL after redirects,
/// file name and whether it supports ranges (= resume + many connections).
/// Some file sites refuse a fresh link for a few seconds (HTTP 403 / 429:
/// the link is made ready after a wait on their page): the program asked
/// once and showed the error, a moment later it worked. It is asked again
/// a few times (2, 3, 5, 8 s) before the refusal is believed.
const NOT_READY_WAITS: [u64; 4] = [2, 3, 5, 8];

pub async fn probe(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
) -> anyhow::Result<ProbeResult> {
    probe_patient(client, url, headers, default_ua, &NOT_READY_WAITS.map(Duration::from_secs)).await
}

/// One look only (the add-in asks while the browser waits): a refusal is
/// told at once.
pub async fn probe_quick(client: &Client, url: &str, headers: &RequestHeaders, default_ua: &str) -> anyhow::Result<ProbeResult> {
    probe_patient(client, url, headers, default_ua, &[]).await
}

pub(super) async fn probe_patient(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
    waits: &[Duration],
) -> anyhow::Result<ProbeResult> {
    // asked a moment ago and its answer kept: not asked again (a link that
    // works only once is not spent by looking at it)
    if let Some(look) = kept_look(url) {
        return Ok(look);
    }
    let mut waits = waits.iter();
    loop {
        let client = &client_for(client, url);
        let answer = match probe_once(client, url, headers, default_ua).await {
            // refused: over IPv4 (a link for the IP the page saw)
            Err(e) if matches!(http_status_of(&e), Some(403)) && !wants_captcha(&e) => match (host_of(url), v4_client()) {
                (Some(host), Some(v4)) if !V4_HOSTS.lock_ok().contains(&host) => match probe_once(&v4, url, headers, default_ua).await {
                    Ok(p) => {
                        V4_HOSTS.lock_ok().insert(host);
                        Ok(p)
                    }
                    Err(_) => Err(e),
                },
                _ => Err(e),
            },
            other => other,
        };
        match answer {
            Err(e) if matches!(http_status_of(&e), Some(403 | 429)) && !wants_captcha(&e) => match waits.next() {
                Some(w) => tokio::time::sleep(*w).await,
                None => return Err(e),
            },
            other => return other,
        }
    }
}

/// One look, following the pages that lead on to the file by themselves.
async fn probe_once(client: &Client, url: &str, headers: &RequestHeaders, default_ua: &str) -> anyhow::Result<ProbeResult> {
    let mut at = url.to_string();
    let mut h = headers.clone();
    let mut led = false;
    for hop in 0..=MAX_PAGE_HOPS {
        let (mut look, answer, next) = probe_at(client, &at, &h, default_ua, hop < MAX_PAGE_HOPS).await?;
        if led {
            look.headers_used = Some(look.headers_used.take().unwrap_or_else(|| h.clone()));
        }
        let Some((wait, next, cookies)) = next else {
            if let Some(answer) = answer {
                keep(url, &look, answer);
            }
            return Ok(look);
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        if let Some(used) = look.headers_used.take() {
            h = used;
        }
        let from = url::Url::parse(&at).ok();
        let same_site = from.as_ref().is_some_and(|f| {
            crate::http_download::site_of(f.as_str()) == crate::http_download::site_of(next.as_str())
                && !(f.scheme() == "https" && next.scheme() != "https")
        });
        if same_site {
            h.cookies = add_cookies(h.cookies.as_deref(), &cookies);
        }
        if let Some(f) = from {
            h.referer = Some(f.to_string());
        }
        at = next.to_string();
        led = true;
    }
    bail!("the link opens one web page after another, not a file")
}
