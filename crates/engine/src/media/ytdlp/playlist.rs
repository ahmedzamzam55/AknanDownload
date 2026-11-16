use super::cookies::with_cookies;
use super::formats::as_u64;
use super::options::{YtOption, MP3_KEY};
use super::subtitles::{playlist_args, ytdlp_command};
use crate::mux::find_tool;
use anyhow::{anyhow, bail, Context};
use serde_json::Value;

/// One video of a playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
    /// seconds (0 = unknown)
    pub duration: u64,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
    /// lists inside it (a channel's tabs): their addresses
    pub tabs: Vec<String>,
}

/// The videos of a playlist (or channel page), without looking at each
/// video yet (`--flat-playlist`, fast). `lang`: the language wanted for the
/// names ("ar"; "" = the site's choice). Blocking.
pub fn playlist(url: &str, lang: &str) -> anyhow::Result<PlaylistInfo> {
    let url = channel_videos(url);
    let mut info = playlist_once(&url, lang)?;
    // An Arabic (Russian…) list where only some videos have a name in the
    // user's language comes mixed: then all in the list's own language
    if let Some(own) = script_lang(&info.title).filter(|own| *own != lang) {
        let theirs = info.entries.iter().filter(|e| script_lang(&e.title) == Some(own)).count();
        if theirs > 0 && theirs < info.entries.len() {
            if let Ok(again) = playlist_once(&url, own) {
                let now = again.entries.iter().filter(|e| script_lang(&e.title) == Some(own)).count();
                if now > theirs && again.entries.len() >= info.entries.len() {
                    info = again;
                }
            }
        }
    }
    // a channel page gives its tabs (Videos, Shorts, Live), not videos: the
    // videos of the first tab
    if info.entries.is_empty() {
        if let Some(tab) = info.tabs.first() {
            let mut inner = playlist_once(tab, lang)?;
            if inner.title == "Playlist" {
                inner.title = info.title;
            }
            return Ok(inner);
        }
    }
    Ok(info)
}

/// The language a name is written in, told by its first letter, for the
/// scripts that belong to one language ("ar" for Arabic letters…); None
/// for Latin letters (many languages) and names without letters.
pub(super) fn script_lang(name: &str) -> Option<&'static str> {
    for c in name.chars() {
        let u = c as u32;
        let lang = match u {
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "ar",
            0x0400..=0x04FF => "ru",
            0x0590..=0x05FF => "iw",
            0x0370..=0x03FF => "el",
            0x0E00..=0x0E7F => "th",
            0x3040..=0x30FF => "ja",
            0xAC00..=0xD7AF => "ko",
            _ if c.is_alphabetic() => return None,
            _ => continue,
        };
        return Some(lang);
    }
    None
}

/// A YouTube channel's own address ("youtube.com/@name") lists its tabs:
/// its videos are under "/videos".
pub(super) fn channel_videos(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let Some(at) = trimmed.find("youtube.com/@") else { return url.trim().to_string() };
    let rest = &trimmed[at + "youtube.com/@".len()..];
    if rest.is_empty() || rest.contains('?') || rest.contains('#') {
        return url.trim().to_string();
    }
    match rest.split_once('/') {
        None => format!("{trimmed}/videos"),
        Some((_, "featured")) => format!("{}/videos", trimmed.trim_end_matches("/featured")),
        Some(_) => url.trim().to_string(),
    }
}

fn playlist_once(url: &str, lang: &str) -> anyhow::Result<PlaylistInfo> {
    let tool = find_tool("yt-dlp").ok_or_else(|| anyhow!("yt-dlp.exe was not found next to AknanDownload.exe"))?;
    let json = with_cookies(url, None, |file| {
        let mut cmd = ytdlp_command(&tool);
        cmd.args(playlist_args(url, file, lang));
        let out = cmd.output().context("could not start yt-dlp")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
            bail!("{}", line.trim().trim_start_matches("ERROR:").trim());
        }
        serde_json::from_slice::<Value>(&out.stdout).context("yt-dlp returned something unexpected")
    })?;
    Ok(parse_playlist(&json))
}

pub fn parse_playlist(json: &Value) -> PlaylistInfo {
    let title = json["title"].as_str().unwrap_or("Playlist").to_string();
    let mut entries = Vec::new();
    let list = json["entries"].as_array().cloned().unwrap_or_default();
    if list.is_empty() && json["_type"].as_str() != Some("playlist") {
        // a single video
        if let Some(u) = json["webpage_url"].as_str().or_else(|| json["original_url"].as_str()) {
            entries.push(PlaylistEntry {
                title: title.clone(),
                url: u.to_string(),
                duration: as_u64(&json["duration"]).unwrap_or(0),
            });
        }
    }
    let mut tabs = Vec::new();
    for e in list {
        // a list inside the list (a channel's "Videos" tab): not a video
        if e["_type"].as_str() == Some("playlist") || e["ie_key"].as_str() == Some("YoutubeTab") {
            if let Some(u) = e["url"].as_str().filter(|u| u.starts_with("http")) {
                tabs.push(u.to_string());
            }
            continue;
        }
        let url = e["url"]
            .as_str()
            .filter(|u| u.starts_with("http"))
            .map(String::from)
            .or_else(|| e["webpage_url"].as_str().map(String::from))
            .or_else(|| e["id"].as_str().map(|id| format!("https://www.youtube.com/watch?v={id}")));
        let Some(url) = url else { continue };
        let t = e["title"].as_str().unwrap_or("video");
        // deleted / private videos
        if t == "[Deleted video]" || t == "[Private video]" {
            continue;
        }
        entries.push(PlaylistEntry { title: t.to_string(), url, duration: as_u64(&e["duration"]).unwrap_or(0) });
    }
    PlaylistInfo { title, entries, tabs }
}

/// Height of a choice ("1080p60" -> 1080; audio / unknown -> 0).
fn height_of(o: &YtOption) -> u64 {
    o.quality.split('p').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Pick a choice by a wanted quality: "best", "audio" or a height like
/// "720" (the best one not above it, MP4 first; the lowest if all are higher).
pub fn pick_option(options: &[YtOption], wanted: &str) -> Option<YtOption> {
    if wanted == "mp3" {
        return options.iter().find(|o| o.key == MP3_KEY).cloned();
    }
    if wanted == "audio" {
        return options.iter().find(|o| o.key.starts_with("audio|") && o.key != MP3_KEY).cloned();
    }
    let videos: Vec<&YtOption> = options.iter().filter(|o| !o.key.starts_with("audio|")).collect();
    if videos.is_empty() {
        return options.first().cloned();
    }
    let limit: u64 = wanted.parse().unwrap_or(u64::MAX);
    let fits: Vec<&YtOption> = videos.iter().copied().filter(|o| height_of(o) <= limit).collect();
    let pool = if fits.is_empty() {
        let low = videos.iter().map(|o| height_of(o)).min().unwrap_or(0);
        videos.iter().copied().filter(|o| height_of(o) == low).collect::<Vec<_>>()
    } else {
        let top = fits.iter().map(|o| height_of(o)).max().unwrap_or(0);
        fits.into_iter().filter(|o| height_of(o) == top).collect()
    };
    pool.iter().find(|o| o.ext == "mp4").or(pool.first()).map(|o| (*o).clone())
}

