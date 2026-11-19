use anyhow::bail;
use serde::Deserialize;
use std::path::Path;
use std::time::SystemTime;

/// A cookie as the browser extension sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    #[serde(default = "root_path")]
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub expiration_date: Option<f64>,
}

fn root_path() -> String {
    "/".into()
}

/// Netscape cookie file (what yt-dlp reads) from the browser's cookies.
pub fn netscape_cookies(cookies: &[BrowserCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        if c.name.is_empty() || c.domain.is_empty() || c.name.contains(['\t', '\n']) || c.value.contains(['\t', '\n']) {
            continue;
        }
        let sub = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let expires = c.expiration_date.map(|e| e.max(0.0) as u64).unwrap_or(2_147_483_647);
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.domain,
            sub,
            if c.path.is_empty() { "/" } else { &c.path },
            if c.secure { "TRUE" } else { "FALSE" },
            expires,
            c.name,
            c.value
        ));
    }
    out
}

/// A new, private file for the browser's cookies (never one that exists).
pub(super) fn cookie_file(cookies: &[BrowserCookie]) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    for _ in 0..5 {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = crate::tools::work_dir().unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!("aknan-cookies-{}-{nanos:x}-{n}.txt", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(netscape_cookies(cookies).as_bytes())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("could not make a cookie file")
}

pub(super) fn needs_login(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["sign in", "log in", "login", "cookies", "age", "bot", "private", "registered users", "rate-limit", "empty media"]
        .iter()
        .any(|k| m.contains(k))
}

/// Sites where much is seen only by friends / group members.
fn members_site(url: &str) -> bool {
    matches!(site_of(url).as_str(), "facebook.com" | "fb.watch" | "instagram.com" | "threads.net" | "x.com" | "twitter.com")
}

/// The site part of an address that cookies belong to ("www.youtube.com",
/// "m.youtube.com" and "youtu.be" are all "youtube.com").
pub(super) fn site_of(url: &str) -> String {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    if host == "youtu.be" || host.ends_with(".youtu.be") || host == "youtubekids.com" || host.ends_with(".youtubekids.com") {
        return "youtube.com".into();
    }
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    // co.uk, com.eg, ...: keep three labels
    let keep = if parts.len() >= 3 && parts[parts.len() - 2].len() <= 3 && parts[parts.len() - 1].len() == 2 { 3 } else { 2 };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

pub(super) fn is_youtube_site(url: &str) -> bool {
    site_of(url) == "youtube.com"
}

pub(super) fn run_with_cookie_list<T>(cookies: &[BrowserCookie], run: impl FnOnce(Option<&Path>) -> anyhow::Result<T>) -> anyhow::Result<T> {
    if cookies.is_empty() {
        return run(None);
    }
    let file = cookie_file(cookies)?;
    let r = run(Some(&file));
    let _ = std::fs::remove_file(&file);
    r
}

/// The browser's cookies of each site, kept in memory only (never written
/// to the disk) while the program runs: a quality picked later, the next
/// video of a playlist or an address typed in "Add URL" can then also open
/// videos that need a login or an age check.
static JAR: std::sync::Mutex<Vec<(String, Vec<BrowserCookie>)>> = std::sync::Mutex::new(Vec::new());

/// Keep the browser's cookies of a site for later (replaces older ones).
pub fn remember_cookies(page_url: &str, cookies: &[BrowserCookie]) {
    if cookies.is_empty() {
        return;
    }
    let site = site_of(page_url);
    if site.is_empty() {
        return;
    }
    let mut jar = JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.retain(|(s, _)| *s != site);
    jar.push((site, cookies.to_vec()));
    if jar.len() > 20 {
        jar.remove(0);
    }
}

/// The cookies the browser gave for this site, if any.
pub fn remembered_cookies(page_url: &str) -> Option<Vec<BrowserCookie>> {
    let site = site_of(page_url);
    let jar = JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.iter().find(|(s, _)| *s == site).map(|(_, c)| c.clone())
}

/// Clearly a login or age wall (not just any error with "age" in it).
pub(super) fn login_wall(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["sign in", "log in", "login", "age-restricted", "age restricted", "confirm your age", "inappropriate for some users", "registered users", "private video", "cookies"]
        .iter()
        .any(|k| m.contains(k))
}

/// The error shown when a video needs a login or an age check and no
/// cookies of the browser help.
pub const LOGIN_NEEDED: &str = "this video needs a login or an age check: open it once in the browser where you are signed in (with the Aknan Download extension), then try again";

pub(super) fn with_cookies<T>(
    page_url: &str,
    cookies: Option<&[BrowserCookie]>,
    run: impl Fn(Option<&Path>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if let Some(list) = cookies {
        remember_cookies(page_url, list);
    }
    match run(None) {
        Ok(v) => Ok(v),
        // social sites hide what only friends / members see behind messages
        // that say nothing of a login ("Cannot parse data", "No video
        // formats found"): with the browser's cookies there, always
        Err(e) if needs_login(&e.to_string()) || (members_site(page_url) && cookies.is_some_and(|c| !c.is_empty())) => {
            let list = cookies.map(<[BrowserCookie]>::to_vec).or_else(|| remembered_cookies(page_url));
            let Some(list) = list.filter(|c| !c.is_empty()) else {
                return Err(if login_wall(&e.to_string()) { e.context(LOGIN_NEEDED) } else { e });
            };
            let file = cookie_file(&list)?;
            let r = run(Some(&file));
            let _ = std::fs::remove_file(&file);
            r.map_err(|e2| if login_wall(&e2.to_string()) { e2.context(LOGIN_NEEDED) } else { e2 })
        }
        Err(e) => Err(e),
    }
}
