use super::build::build_options;
use super::cookies::{with_cookies, BrowserCookie};
use super::options::{YtInfo, YtOption};
use super::subtitles::{run_ytdlp, ytdlp_command};
use crate::mux::find_tool;
use anyhow::{anyhow, bail, Context};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Find the download choices of a page with a video. Blocking (takes a few
/// seconds). Tries without cookies first, and with the browser's cookies for
/// that site only when the site asks for a login (Instagram, Facebook,
/// age-limited YouTube videos, ...). Cookies the browser gave earlier for
/// the same site are used when none come with this call.
pub fn resolve(page_url: &str, cookies: Option<&[BrowserCookie]>) -> anyhow::Result<YtInfo> {
    resolve_for(page_url, cookies, None)
}

/// The same, for a stream found on a page (`referer`: that page).
pub fn resolve_for(page_url: &str, cookies: Option<&[BrowserCookie]>, referer: Option<&str>) -> anyhow::Result<YtInfo> {
    // the same page again within a few minutes (the add-in asks again when
    // the tab comes back, a playlist video is picked): no new yt-dlp run,
    // which takes seconds and much memory
    if let Some(info) = cached(page_url) {
        return Ok(info);
    }
    let info = resolve_now(page_url, cookies, referer)?;
    remember(page_url, &info);
    Ok(info)
}

/// Results of the last minutes (the links in them stay good for hours).
static RESULTS: std::sync::Mutex<Vec<(String, std::time::Instant, YtInfo)>> = std::sync::Mutex::new(Vec::new());
const KEEP_RESULTS: Duration = Duration::from_secs(10 * 60);

fn cached(page_url: &str) -> Option<YtInfo> {
    let mut list = RESULTS.lock().unwrap_or_else(|p| p.into_inner());
    list.retain(|(_, at, _)| at.elapsed() < KEEP_RESULTS);
    list.iter().find(|(u, _, _)| u == page_url).map(|(_, _, i)| i.clone())
}

fn remember(page_url: &str, info: &YtInfo) {
    let mut list = RESULTS.lock().unwrap_or_else(|p| p.into_inner());
    list.retain(|(u, _, _)| u != page_url);
    list.push((page_url.to_string(), std::time::Instant::now(), info.clone()));
    if list.len() > 30 {
        list.remove(0);
    }
}

/// Forget a page's result: its links stopped working (fresh ones wanted).
pub fn forget(page_url: &str) {
    RESULTS.lock().unwrap_or_else(|p| p.into_inner()).retain(|(u, _, _)| u != page_url);
}

fn resolve_now(page_url: &str, cookies: Option<&[BrowserCookie]>, referer: Option<&str>) -> anyhow::Result<YtInfo> {
    // a file on Google Drive: its own file, whole, as it was put there (the
    // player's qualities are made smaller by Google, and yt-dlp often can't
    // see a file shared with the user)
    let drive = drive_file_id(page_url);
    let tool = find_tool("yt-dlp")
        .ok_or_else(|| anyhow!("yt-dlp.exe is missing. Aknan Download gets it by itself in a minute (internet needed), then try again"));
    let cookies = cookies.filter(|c| !c.is_empty());
    let found = tool.and_then(|tool| with_cookies(page_url, cookies, |file| run_ytdlp(&tool, page_url, file, referer)));
    let mut info = match (found, &drive) {
        (Ok(json), _) => build_options(&json),
        (Err(_), Some(_)) => YtInfo { title: String::new(), site: "Google Drive".into(), options: Vec::new() },
        (Err(e), None) => return Err(e),
    };
    if let Some(id) = drive {
        info.site = "Google Drive".into();
        info.options.insert(0, drive_original(&id, &info));
    }
    if info.options.is_empty() {
        bail!("no downloadable video was found on this page");
    }
    Ok(info)
}

/// The id of a Google Drive file from its page ("/file/d/ID/view",
/// "open?id=ID", "uc?id=ID").
pub fn drive_file_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    if !(host == "drive.google.com" || host == "docs.google.com" || host == "drive.usercontent.google.com") {
        return None;
    }
    let mut parts = u.path_segments()?;
    let id = loop {
        match parts.next() {
            Some("d") => break parts.next().map(str::to_string),
            Some(_) => continue,
            None => break u.query_pairs().find(|(k, _)| k == "id").map(|(_, v)| v.to_string()),
        }
    }?;
    (id.len() >= 10 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(id)
}

/// "Original file": the file itself, as the Download button of Drive gives it.
pub(super) fn drive_original(id: &str, info: &YtInfo) -> YtOption {
    let ext = info.options.first().map(|o| o.ext.clone()).unwrap_or_default();
    YtOption {
        key: "drive-original".into(),
        label: "Original file".into(),
        sub: "Google Drive".into(),
        quality: "Original".into(),
        // confirm=t: no "can't scan this file for viruses" page for big files
        video_url: format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t"),
        audio_url: None,
        ext,
        size: 0,
        headers: vec![("Referer".into(), "https://drive.google.com/".into())],
        tag: String::new(),
        codec: String::new(),
        duration: info.options.first().map(|o| o.duration).unwrap_or(0),
        kbps: 0,
    }
}

/// Fresh links for a choice made earlier (links expire after a few hours).
pub fn resolve_key(page_url: &str, key: &str) -> anyhow::Result<YtOption> {
    forget(page_url);
    let options = resolve(page_url, None)?.options;
    // a choice saved by an older version: "2160p|MP4" may now be "2160p|MP4 AV1"
    let alt = key.strip_suffix(" AV1").map(String::from).unwrap_or_else(|| format!("{key} AV1"));
    options
        .iter()
        .find(|o| o.key == key)
        .or_else(|| options.iter().find(|o| o.key == alt))
        .cloned()
        .ok_or_else(|| anyhow!("this quality is not offered any more"))
}

/// Update yt-dlp right now ("Help > Update video downloader"). Blocking.
pub fn update_now() -> anyhow::Result<String> {
    let tool = find_tool("yt-dlp").ok_or_else(|| anyhow!("yt-dlp.exe was not found next to AknanDownload.exe"))?;
    let mut cmd = ytdlp_command(&tool);
    cmd.args(["--ignore-config", "-U"]);
    let out = cmd.output().context("could not start yt-dlp")?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("done").trim().to_string();
    if out.status.success() {
        Ok(last)
    } else {
        bail!("{last}")
    }
}

/// yt-dlp needs updates when sites change things. Runs `yt-dlp -U` at most
/// once a day, for the copy next to the app. Blocking; call it from a
/// background thread.
pub fn update_if_due(data_dir: &Path) {
    let stamp = data_dir.join("yt-dlp-updated");
    let due = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age > Duration::from_secs(24 * 3600))
        .unwrap_or(true);
    if !due {
        return;
    }
    let Some(tool) = find_tool("yt-dlp") else { return };
    if !tool.is_absolute() {
        return; // installed elsewhere (e.g. winget): leave updates to that
    }
    let mut cmd = ytdlp_command(&tool);
    cmd.args(["--ignore-config", "-U"]);
    if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
        let _ = std::fs::write(&stamp, b"ok");
    }
}
