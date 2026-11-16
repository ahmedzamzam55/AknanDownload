use crate::model::ProbeResult;
use crate::sync::LockOk;
use reqwest::StatusCode;
use std::sync::Mutex;
use std::time::Duration;

// ---------------------------------------------------------------- answers kept for the download

/// The first answer about a file is kept open (its data not read yet) for a
/// while: the download goes on from it instead of asking again. A link that
/// works only once (or for one request) was spent by the look of "Download
/// File Info", and the download itself got "403" or a web page.
pub const KEEP_FOR: Duration = Duration::from_secs(600);
/// At most this many answers are kept at once (each holds a connection).
const KEEP_MAX: usize = 6;

struct Kept {
    url: String,
    at: std::time::Instant,
    look: ProbeResult,
    answer: reqwest::Response,
}

static KEPT: std::sync::LazyLock<Mutex<Vec<Kept>>> = std::sync::LazyLock::new(Default::default);

fn kept_list() -> std::sync::MutexGuard<'static, Vec<Kept>> {
    let mut k = KEPT.lock_ok();
    k.retain(|x| x.at.elapsed() < KEEP_FOR);
    k
}

pub(super) fn keep(url: &str, look: &ProbeResult, answer: reqwest::Response) {
    let mut k = kept_list();
    k.retain(|x| x.url != url);
    k.push(Kept { url: url.to_string(), at: std::time::Instant::now(), look: look.clone(), answer });
    while k.len() > KEEP_MAX {
        k.remove(0);
    }
    drop(k);
    // its connection is let go when nobody took it in time
    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        let url = url.to_string();
        rt.spawn(async move {
            tokio::time::sleep(KEEP_FOR + Duration::from_secs(1)).await;
            drop(kept_list());
            let _ = url;
        });
    }
}

/// What the kept answer for `url` said (it stays kept).
pub(super) fn kept_look(url: &str) -> Option<ProbeResult> {
    kept_list().iter().find(|x| x.url == url).map(|x| x.look.clone())
}

/// The kept answer for `url`, taken: the download reads the file from it.
pub fn take_kept(url: &str) -> Option<(ProbeResult, reqwest::Response)> {
    let mut k = kept_list();
    let i = k.iter().position(|x| x.url == url)?;
    let x = k.remove(i);
    Some((x.look, x.answer))
}

/// Nobody will take the kept answer for `url` (the window was closed, the
/// download goes on from parts already there): its connection goes.
pub fn drop_kept(url: &str) {
    kept_list().retain(|x| x.url != url);
}

// ---------------------------------------------------------------- pages that lead on to the file

/// Most pages in a row a look follows ("your download starts in 5 seconds").
pub(super) const MAX_PAGE_HOPS: usize = 5;
/// The longest wait such a page asks for that is kept.
pub(super) const PAGE_WAIT: Duration = Duration::from_secs(10);
/// How much of a web page is read to find where it leads.
pub(super) const PAGE_PEEK: usize = 256 * 1024;

/// A site's "not a robot" check turned the request away (it says so in a
/// header of its answer).
pub fn is_bot_check(resp: &reqwest::Response) -> bool {
    resp.status() == StatusCode::FORBIDDEN
        && resp
            .headers()
            .get("cf-mitigated")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("challenge"))
}

/// Where a "Refresh" value leads and after how long: "5; url=next.html",
/// "0;URL='https://...'", "3, https://...". None for a page that only
/// reloads itself, or for anything but a web address.
pub fn refresh_target(value: &str, base: &url::Url) -> Option<(Duration, url::Url)> {
    let (delay, rest) = value.split_once([';', ','])?;
    let delay: f64 = delay.trim().parse().ok().filter(|d: &f64| d.is_finite() && *d >= 0.0)?;
    let rest = rest.trim();
    let target = match rest.get(..3) {
        Some(u) if u.eq_ignore_ascii_case("url") && rest[3..].trim_start().starts_with('=') => rest[3..].trim_start()[1..].trim(),
        _ => rest,
    };
    let target = target.trim_matches(|c| c == '"' || c == '\'').trim();
    let target = target.replace("&amp;", "&");
    let next = base.join(&target).ok()?;
    let mut same = next.clone();
    same.set_fragment(base.fragment());
    (matches!(next.scheme(), "http" | "https") && same != *base).then(|| (Duration::from_secs_f64(delay.min(PAGE_WAIT.as_secs_f64())), next))
}

/// The value of attribute `name` in one tag's text (`<meta a="b" ...>`).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // a whole attribute name, not the end of another one
        if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(value.to_string());
    }
    None
}

/// Where a page's `<meta http-equiv="refresh" content="...">` leads.
pub fn meta_refresh(page: &str, base: &url::Url) -> Option<(Duration, url::Url)> {
    let lower = page.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &page[start..end];
        from = end;
        let refresh = attribute(tag, "http-equiv").is_some_and(|v| v.trim().eq_ignore_ascii_case("refresh"));
        if refresh {
            if let Some(found) = attribute(tag, "content").and_then(|c| refresh_target(&c, base)) {
                return Some(found);
            }
        }
    }
    None
}

/// The "name=value" pairs a response's Set-Cookie headers set.
pub(super) fn set_cookies(h: &reqwest::header::HeaderMap) -> Vec<String> {
    h.get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .map(str::trim)
        .filter(|c| c.contains('='))
        .map(str::to_string)
        .collect()
}

/// `cookies` added to `old` (each replacing one of the same name).
pub fn add_cookies(old: Option<&str>, cookies: &[String]) -> Option<String> {
    if cookies.is_empty() {
        return old.map(str::to_string);
    }
    let name = |c: &str| c.split('=').next().unwrap_or("").trim().to_string();
    let new: std::collections::HashSet<String> = cookies.iter().map(|c| name(c)).collect();
    let mut all: Vec<String> = old
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|c| !c.is_empty() && !new.contains(&name(c)))
        .map(str::to_string)
        .collect();
    all.extend(cookies.iter().cloned());
    Some(all.join("; "))
}
