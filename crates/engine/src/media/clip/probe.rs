use super::Picture;
use crate::sync::LockOk;
use anyhow::{anyhow, bail, Context};
use std::path::Path;
use std::process::Stdio;
use tokio_util::sync::CancellationToken;

pub fn parse_picture(ffmpeg_says: &str) -> Option<Picture> {
    let line = ffmpeg_says.lines().find(|l| l.contains("Stream #") && l.contains(": Video: "))?;
    let after = line.split(": Video: ").nth(1)?;
    let codec = after.split([' ', ',']).next().unwrap_or("").to_string();
    let size = after
        .split([',', ' '])
        .map(str::trim)
        .find_map(|w| {
            let (a, b) = w.split_once('x')?;
            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
        })
        .unwrap_or((0, 0));
    Some(Picture { codec, width: size.0, height: size.1 })
}

/// "Duration: 00:03:25.48," -> 205.48
pub fn parse_duration(ffmpeg_says: &str) -> Option<f64> {
    let at = ffmpeg_says.find("Duration: ")? + "Duration: ".len();
    let t = ffmpeg_says[at..].split(',').next()?.trim();
    let mut secs = 0.0;
    for p in t.split(':') {
        secs = secs * 60.0 + p.trim().parse::<f64>().ok()?;
    }
    (secs > 0.0).then_some(secs)
}

/// Kinds of picture our ffmpeg reads (to make them again for the exact cut).
pub(super) fn readable(codec: &str) -> bool {
    matches!(codec, "h264" | "hevc" | "vp8" | "vp9" | "av1" | "mpeg4")
}

/// The H.264 maker of this ffmpeg (ours has OpenH264).
pub(crate) fn h264_maker(ffmpeg: &Path) -> &'static str {
    static FOUND: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FOUND.get_or_init(|| {
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-encoders"]);
        crate::mux::hide_window(&mut cmd);
        let list = cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        ["libopenh264", "libx264", "h264_mf"].into_iter().find(|e| list.contains(&format!(" {e} "))).unwrap_or("libopenh264")
    })
}

/// Picture quality of the exact cut (kb/s) for a picture of this size.
pub(crate) fn video_kbps(p: &Picture) -> u64 {
    let pixels = (p.width as u64 * p.height as u64).max(640 * 360);
    (pixels * 8000 / (1920 * 1080)).clamp(1200, 40_000)
}

pub(super) fn command(ffmpeg: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(ffmpeg);
    crate::mux::hide_window(&mut cmd);
    crate::http::with_proxy(&mut cmd);
    cmd.stdin(Stdio::null());
    cmd
}

/// What ffmpeg sees in `input`: how long it plays and its picture.
pub async fn look(ffmpeg: &Path, input: &str) -> anyhow::Result<(Option<f64>, Option<Picture>)> {
    let (ffmpeg, input) = (ffmpeg.to_path_buf(), input.to_string());
    let out = tokio::task::spawn_blocking(move || {
        command(&ffmpeg).args(["-hide_banner", "-nostdin", "-i", &input]).stdout(Stdio::null()).stderr(Stdio::piped()).output()
    })
    .await?
    .context("could not start ffmpeg")?;
    // "At least one output file must be specified": that is fine here
    let said = String::from_utf8_lossy(&out.stderr);
    if !said.contains("Input #0") {
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not read the video: {last}");
    }
    Ok((parse_duration(&said), parse_picture(&said)))
}

/// Run ffmpeg with `args` + `output`; `done(seconds made)` while it works;
/// stopped at once when `cancel` fires.
pub(super) async fn run_ffmpeg(ffmpeg: &Path, args: Vec<String>, output: &Path, done: &(dyn Fn(f64) + Sync), cancel: &CancellationToken) -> anyhow::Result<()> {
    use std::io::{BufRead, Read};
    let mut child = command(ffmpeg).args(&args).arg(output).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("could not start ffmpeg")?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("ffmpeg gave no output"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("ffmpeg gave no output"))?;
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    // how far it is, from its "-progress" lines
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<f64>();
    let reader = std::thread::spawn(move || {
        for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = l.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<i64>().ok()) {
                let _ = tx.send(us.max(0) as f64 / 1e6);
            }
        }
    });
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    loop {
        tokio::select! {
            got = rx.recv() => match got {
                Some(secs) => done(secs),
                None => break,
            },
            _ = cancel.cancelled() => {
                let _ = child.lock_ok().kill();
                let _ = child.lock_ok().wait();
                return Err(crate::http_download::Cancelled.into());
            }
        }
    }
    let _ = reader.join();
    let c2 = child.clone();
    let status = tokio::task::spawn_blocking(move || c2.lock_ok().wait()).await??;
    let said = errors.join().unwrap_or_default();
    let made = std::fs::metadata(output).map(|m| m.len() > 0).unwrap_or(false);
    if !status.success() || !made {
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").to_string();
        bail!("ffmpeg could not cut the part: {last}");
    }
    Ok(())
}

