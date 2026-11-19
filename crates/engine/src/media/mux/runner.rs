use super::{find_ffmpeg, format_for, MISSING};
use anyhow::{bail, Context};
use std::path::Path;
use std::process::Command;

/// Files from this size on are written once only: the "plays while still
/// loading" order of an MP4 file (+faststart) writes the whole file a second
/// time, minutes for a big video, and a file on the computer plays and
/// winds forward as well without it.
const FASTSTART_UP_TO: u64 = 1 << 30;

pub(super) fn len_of(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Run ffmpeg (`cmd`, its output file last) and tell how far it is: how much
/// of `expected` bytes `output` has. Stopped at once when `cancel` fires.
/// Gives what ffmpeg said when it failed.
pub fn run_watched(
    cmd: &mut Command,
    output: &Path,
    expected: u64,
    on: &dyn Fn(f64),
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> anyhow::Result<()> {
    use std::io::Read;
    use std::process::Stdio;
    hide_window(cmd);
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().context("could not start ffmpeg")?;
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if cancel.is_some_and(|c| c.is_cancelled()) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = errors.join();
            return Err(crate::http_download::Cancelled.into());
        }
        if expected > 0 {
            on((len_of(output) as f64 / expected as f64).min(0.99));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    };
    let said = errors.join().unwrap_or_default();
    if !status.success() {
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").to_string();
        bail!("{last}");
    }
    on(1.0);
    Ok(())
}

pub fn merge(
    video: &Path,
    audio: &Path,
    output: &Path,
    final_name: &str,
    on: &dyn Fn(f64),
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let format = format_for(final_name);
    let expected = len_of(video) + len_of(audio);
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(video)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);
    if format == "mp4" && expected < FASTSTART_UP_TO {
        // play while still loading, like normal web videos
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-f", format]).arg(output);
    run_watched(&mut cmd, output, expected, on, cancel).map_err(|e| match e.downcast::<crate::http_download::Cancelled>() {
        Ok(c) => anyhow::Error::new(c),
        Err(e) => anyhow::anyhow!("ffmpeg could not merge video and audio: {e}"),
    })
}


fn build_hls_cmd(ffmpeg: &Path, input: &Path, audio: Option<&Path>, output: &Path, fmp4: bool, bsf_aac: bool, expected: u64) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    if !fmp4 {
        cmd.args(["-fflags", "+genpts+igndts", "-avoid_negative_ts", "make_zero", "-f", "mpegts"]);
    }
    cmd.arg("-i").arg(input);
    if let Some(a) = audio {
        if !fmp4 {
            cmd.args(["-fflags", "+genpts+igndts"]);
        }
        cmd.arg("-i").arg(a).args(["-map", "0:v:0", "-map", "1:a:0"]);
    } else {
        cmd.args(["-map", "0:v?", "-map", "0:a?"]);
    }
    cmd.args(["-c", "copy"]);
    if !fmp4 && bsf_aac {
        cmd.args(["-bsf:a", "aac_adtstoasc"]);
    }
    if expected < FASTSTART_UP_TO {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-f", "mp4"]).arg(output);
    cmd
}

/// Remux an HLS stream (`MpegTs` or `Fmp4`, with optional separate audio) into
/// a seekable MP4 file without re-encoding. Retries non-AAC TS audio cleanly.
pub fn remux_hls(
    input: &Path,
    audio: Option<&Path>,
    output: &Path,
    fmp4: bool,
    on: &dyn Fn(f64),
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let expected = (len_of(input) + audio.map(len_of).unwrap_or(0)) * 95 / 100;
    let mut cmd = build_hls_cmd(&ffmpeg, input, audio, output, fmp4, !fmp4, expected);
    let mut r = run_watched(&mut cmd, output, expected, on, cancel);
    let mut made = std::fs::metadata(output).map(|m| m.len() > 0).unwrap_or(false);
    if (!fmp4) && (r.is_err() || !made) && !cancel.is_some_and(|c| c.is_cancelled()) {
        let _ = std::fs::remove_file(output);
        let mut fallback = build_hls_cmd(&ffmpeg, input, audio, output, false, false, expected);
        r = run_watched(&mut fallback, output, expected, on, cancel);
        made = std::fs::metadata(output).map(|m| m.len() > 0).unwrap_or(false);
    }
    match r {
        Ok(()) if made => Ok(()),
        Ok(()) => {
            let _ = std::fs::remove_file(output);
            bail!("ffmpeg could not make the MP4 file: nothing was written")
        }
        Err(e) => {
            let _ = std::fs::remove_file(output);
            if e.is::<crate::http_download::Cancelled>() {
                return Err(e);
            }
            bail!("ffmpeg could not make the MP4 file: {e}")
        }
    }
}

/// A joined stream (HLS, a .ts file) into an ordinary MP4 file: plays in
/// every player, with seeking. Nothing is re-encoded.
pub fn ts_to_mp4(input: &Path, output: &Path, on: &dyn Fn(f64), cancel: Option<&tokio_util::sync::CancellationToken>) -> anyhow::Result<()> {
    remux_hls(input, None, output, false, on, cancel)
}

pub(crate) fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}
