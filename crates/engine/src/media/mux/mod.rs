//! Joining a video-only and an audio-only file into one playable file.
//! Uses ffmpeg in "copy" mode: no re-encoding, so it takes seconds and keeps
//! the original quality.

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A helper program (ffmpeg, yt-dlp): the copy in the app's "tools"
/// folder wins, then one next to AknanDownload.exe, then PATH.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    if let Some(p) = crate::tools::bundled(name) {
        return Some(p);
    }
    if let Ok(me) = std::env::current_exe() {
        let local = me.with_file_name(&exe_name);
        if local.exists() {
            return Some(local);
        }
        let in_folder = me.with_file_name(name).join("bin").join(&exe_name);
        if in_folder.exists() {
            return Some(in_folder);
        }
    }
    let mut probe = Command::new(&exe_name);
    probe.arg("--version");
    hide_window(&mut probe);
    match probe.output() {
        Ok(o) if o.status.success() => Some(PathBuf::from(exe_name)),
        // ffmpeg prints its version with -version and fails on --version
        Ok(_) if name == "ffmpeg" => {
            let mut p = Command::new(&exe_name);
            p.arg("-version");
            hide_window(&mut p);
            p.output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from(&exe_name))
        }
        _ => None,
    }
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    find_tool("ffmpeg")
}

/// ffmpeg muxer name for the final file extension. The output path ends in
/// `.sgpart` while downloading, so ffmpeg can't guess it from the name.
pub fn format_for(file_name: &str) -> &'static str {
    match crate::filename::extension(file_name).as_str() {
        "webm" => "webm",
        "mkv" => "matroska",
        "m4a" | "mp4" | "m4v" | "mov" => "mp4",
        _ => "matroska",
    }
}

/// Files from this size on are written once only: the "plays while still
/// loading" order of an MP4 file (+faststart) writes the whole file a second
/// time, minutes for a big video, and a file on the computer plays and
/// winds forward as well without it.

mod runner;
#[cfg(test)]
mod tests;

pub use runner::{merge, run_watched, ts_to_mp4};
pub(crate) use runner::hide_window;
use runner::len_of;
/// What to say when our ffmpeg.exe is not there (it comes with the setup).
pub const MISSING: &str = "ffmpeg.exe is missing from the \"tools\" folder of Aknan Download and could not be downloaded again (no internet, or an antivirus removes it): install Aknan Download again, or allow ffmpeg.exe in the antivirus";

/// "Play while downloading" for picture + sound in two files: one playable
/// file of what is there so far (the first `video_ready` / `audio_ready`
/// bytes, nothing is re-encoded, takes a moment). `output` gets a
/// Matroska (.mkv) file: it takes every kind of picture and sound.
pub fn preview_snapshot(video: &Path, video_ready: u64, audio: Option<(&Path, u64)>, output: &Path) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    // only the part with no gap ("subfile": ffmpeg reads just those bytes)
    let part = |p: &Path, n: u64| {
        let mut s = std::ffi::OsString::from(format!("subfile,,start,0,end,{n},,:"));
        s.push(p.as_os_str());
        s
    };
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-protocol_whitelist", "file,subfile", "-i"])
        .arg(part(video, video_ready));
    if let Some((a, n)) = audio {
        cmd.arg("-i").arg(part(a, n)).args(["-map", "0:v:0", "-map", "1:a:0"]);
    } else {
        cmd.args(["-map", "0:v:0"]);
    }
    cmd.args(["-c", "copy", "-f", "matroska"]).arg(output);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start ffmpeg")?;
    // the cut at the end makes ffmpeg complain; what it wrote is fine
    let made = std::fs::metadata(output).map(|m| m.len() > 1024).unwrap_or(false);
    if !made {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not prepare the preview: {last}");
    }
    Ok(())
}

/// Turn a downloaded sound (or video) file into an MP3 file.
pub fn to_mp3(input: &Path, output: &Path, kbps: u64, title: Option<&str>) -> anyhow::Result<()> {
    to_mp3_watched(input, output, kbps, title, &|_| {}, None)
}

/// The same, telling how far it is.
pub fn to_mp3_watched(
    input: &Path,
    output: &Path,
    kbps: u64,
    title: Option<&str>,
    on: &dyn Fn(f64),
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    // the sound usually comes at about 128 kb/s
    let expected = len_of(input) * kbps.max(32) / 128;
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-map", "0:a:0", "-map_metadata", "-1", "-c:a", "libmp3lame", "-b:a"])
        .arg(format!("{kbps}k"))
        // the name shows on phones, car radios and players
        .args(["-id3v2_version", "3"]);
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        cmd.arg("-metadata").arg(format!("title={}", t.chars().take(200).collect::<String>()));
    }
    cmd.args(["-f", "mp3"]).arg(output);
    run_watched(&mut cmd, output, expected, on, cancel).map_err(|e| match e.downcast::<crate::http_download::Cancelled>() {
        Ok(c) => anyhow::Error::new(c),
        Err(e) => anyhow::anyhow!("ffmpeg could not make the MP3 file: {e}"),
    })
}

