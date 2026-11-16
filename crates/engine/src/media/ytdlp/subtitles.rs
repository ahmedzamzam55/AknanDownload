use super::cookies::with_cookies;
use crate::mux::{find_tool, hide_window};
use anyhow::{anyhow, bail, Context};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// The arguments for looking at one page. `--ignore-config`: settings files
/// of yt-dlp on the computer (which could add `--exec` and the like) are not
/// read; `--`: the address can never be taken for an option.
pub(super) fn resolve_args(page_url: &str, cookie_file: Option<&Path>, referer: Option<&str>) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["--ignore-config", "-J", "--no-playlist", "--no-warnings", "--no-progress", "--no-cache-dir", "--socket-timeout", "20"]
        .iter()
        .map(Into::into)
        .collect();
    if let Some(c) = cookie_file {
        a.push("--cookies".into());
        a.push(c.into());
    }
    // the page a stream was found on (many video servers want it)
    if let Some(r) = referer.filter(|r| r.starts_with("https://") || r.starts_with("http://")) {
        a.push("--referer".into());
        a.push(r.into());
    }
    a.push("--".into());
    a.push(page_url.into());
    a
}

pub(super) fn playlist_args(url: &str, cookie_file: Option<&Path>, lang: &str) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["--ignore-config", "-J", "--flat-playlist", "--yes-playlist", "--no-warnings", "--no-cache-dir", "--socket-timeout", "20"]
        .iter()
        .map(Into::into)
        .collect();
    // the names in the user's language, when the site has them in it
    if !lang.is_empty() && lang.len() <= 5 && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        a.push("--extractor-args".into());
        a.push(format!("youtube:lang={lang}").into());
    }
    if let Some(c) = cookie_file {
        a.push("--cookies".into());
        a.push(c.into());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

/// yt-dlp, started so that its own temporary files go to our folder: on
/// some computers the Windows temporary folder is on a drive Windows does
/// not let programs walk through ("WinError 448 ... untrusted mount point").
pub(super) fn ytdlp_command(tool: &Path) -> Command {
    let mut cmd = Command::new(tool);
    if let Some(dir) = crate::tools::work_dir() {
        cmd.env("TEMP", &dir).env("TMP", &dir).current_dir(&dir);
    }
    crate::http::with_proxy(&mut cmd);
    hide_window(&mut cmd);
    cmd
}

pub(super) fn run_ytdlp(tool: &Path, page_url: &str, cookie_file: Option<&Path>, referer: Option<&str>) -> anyhow::Result<Value> {
    let mut cmd = ytdlp_command(tool);
    cmd.args(resolve_args(page_url, cookie_file, referer));
    let out = cmd.output().context("could not start yt-dlp")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| err.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("unknown error")
            .trim()
            .trim_start_matches("ERROR:")
            .trim()
            .to_string();
        bail!("{line}");
    }
    serde_json::from_slice(&out.stdout).context("yt-dlp returned something unexpected")
}

/// The subtitles of a video page (when the site has them), saved next to
/// the video as "<name>.<language>.<ext>". `langs`: "ar en" (yt-dlp's own
/// automatic translations are used when there are no real ones). Blocking.
pub fn fetch_subtitles(page_url: &str, dir: &Path, stem: &str, langs: &str) -> anyhow::Result<()> {
    let tool = find_tool("yt-dlp").ok_or_else(|| anyhow!("yt-dlp.exe is missing"))?;
    let langs: Vec<String> = langs
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && l.len() <= 12 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .collect();
    if langs.is_empty() {
        return Ok(());
    }
    // "ar" also takes "ar-EG" and the like
    let wanted = langs.iter().map(|l| format!("{l}.*")).chain(langs.iter().cloned()).collect::<Vec<_>>().join(",");
    // a name of ours as the output template: "%" means something to yt-dlp
    let out = format!("{}.%(ext)s", stem.replace('%', "%%"));
    with_cookies(page_url, None, |file| {
        let mut cmd = ytdlp_command(&tool);
        cmd.args(["--ignore-config", "--skip-download", "--write-subs", "--write-auto-subs", "--no-playlist", "--no-warnings", "--no-cache-dir"])
            .args(["--socket-timeout", "20", "--sub-format", "srt/vtt/best", "--sub-langs"])
            .arg(&wanted)
            .arg("-P")
            .arg(dir)
            .arg("-o")
            .arg(&out);
        if let Some(c) = file {
            cmd.arg("--cookies").arg(c);
        }
        cmd.arg("--").arg(page_url);
        let o = cmd.output().context("could not start yt-dlp")?;
        if o.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&o.stderr);
            bail!("{}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no subtitles"))
        }
    })
}
