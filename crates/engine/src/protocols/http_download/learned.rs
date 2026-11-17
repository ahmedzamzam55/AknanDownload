use super::types::{SegTable, TAIL_SPLIT};
use crate::http::Progress;
use crate::sync::LockOk;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// Sites that answered "too many requests" (HTTP 429): how many connections
/// they took, and when that was seen (seconds since 1970). Their next
/// downloads start with that many, for a day: a site that was busy once
/// (or a big shared server, a CDN) is not held at one or two connections
/// for ever.
/// site -> (connections, when); and whether it changed since it was kept
type Learned = (std::collections::BTreeMap<String, (u32, u64)>, bool);
static LEARNED: std::sync::LazyLock<Mutex<Learned>> =
    std::sync::LazyLock::new(|| Mutex::new((Default::default(), false)));

/// How long a learned limit holds.
const LEARNED_FOR: u64 = 24 * 3600;

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The site of an address: download2430.mediafire.com -> mediafire.com
/// (its many download servers count as one).
pub fn site_of(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return Some(host);
    }
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    let keep = match parts.as_slice() {
        // example.co.uk, example.com.eg
        [.., second, last] if last.len() == 2 && matches!(*second, "co" | "com" | "net" | "org" | "gov" | "edu" | "ac") => 3,
        _ => 2,
    };
    Some(parts[parts.len().saturating_sub(keep)..].join("."))
}

/// How many connections the site of `url` took (None: no limit seen lately).
pub fn learned_limit(url: &str) -> Option<u32> {
    let site = site_of(url)?;
    let l = LEARNED.lock_ok();
    l.0.get(&site).filter(|(_, at)| now_secs().saturating_sub(*at) < LEARNED_FOR).map(|(n, _)| *n)
}

pub(crate) fn learn(url: &str, n: u32) {
    let Some(site) = site_of(url) else { return };
    let mut l = LEARNED.lock_ok();
    let n = n.max(1);
    let now = now_secs();
    let lower = match l.0.get(&site) {
        None => true,
        Some((old, at)) => n < *old || now.saturating_sub(*at) >= LEARNED_FOR,
    };
    if lower {
        l.0.insert(site, (n, now));
        l.1 = true;
    }
}

/// Record a higher proven concurrency when a site safely handles more streams.
pub(crate) fn raise_learned(url: &str, n: u32) {
    let Some(site) = site_of(url) else { return };
    let mut l = LEARNED.lock_ok();
    if let Some((old, _)) = l.0.get(&site) {
        if n > *old {
            l.0.insert(site, (n, now_secs()));
            l.1 = true;
        }
    }
}

/// The limits seen before (at start), as kept by `take_learned_changes`:
/// {"site": [n, seconds]}. Older files ({"site": n}, no time) are not taken:
/// their limits were kept for ever, and are let go now.
pub fn set_learned(map: std::collections::BTreeMap<String, serde_json::Value>) {
    let now = now_secs();
    let mut l = LEARNED.lock_ok();
    l.0 = map
        .into_iter()
        .filter_map(|(site, v)| {
            let n = v.get(0)?.as_u64()? as u32;
            let at = v.get(1)?.as_u64()?;
            (n >= 1 && now.saturating_sub(at) < LEARNED_FOR).then_some((site, (n, at)))
        })
        .collect();
    l.1 = false;
}

/// The limits, when new ones were seen since the last call (to be kept).
pub fn take_learned_changes() -> Option<std::collections::BTreeMap<String, (u32, u64)>> {
    let mut l = LEARNED.lock_ok();
    if !l.1 {
        return None;
    }
    l.1 = false;
    let now = now_secs();
    Some(l.0.iter().filter(|(_, (_, at))| now.saturating_sub(*at) < LEARNED_FOR).map(|(k, v)| (k.clone(), *v)).collect())
}

/// Fewer connections from now on (never more than wanted already).
pub(super) fn lower_wanted(progress: &Progress, job_conns: u32, n: u32) {
    let n = n.max(1);
    progress.lowered_at.store(now_secs(), Ordering::Relaxed);
    let _ = progress.want_connections.try_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        let now = if cur == 0 { job_conns } else { cur };
        (n < now).then_some(n)
    });
}

/// "Retry-After: 30" -> 30 s (a date or nothing: None).
pub(super) fn retry_after_of(resp: &reqwest::Response) -> Option<u64> {
    resp.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse::<u64>().ok().filter(|s| *s <= 600)
}

/// A part could be taken by one more connection: one nobody works on, or
/// one big enough to be shared.
pub(super) fn has_work(segs: &SegTable, min_split: u64) -> bool {
    let t = segs.lock_ok();
    let min = min_split.clamp(1, TAIL_SPLIT) * 2;
    t.iter().any(|s| !s.is_finished() && (!s.active || s.remaining() >= min))
}
