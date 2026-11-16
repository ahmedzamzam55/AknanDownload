use super::runner::{len_of, run_watched};
use super::{find_ffmpeg, format_for, MISSING};
use anyhow::bail;
use std::path::{Path, PathBuf};
use std::process::Command;

const FASTSTART_UP_TO: u64 = 1 << 30;

struct TempSubFile(PathBuf);

impl Drop for TempSubFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn abs_path(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(p)).unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Burn SRT subtitles directly onto the video frames and write the muxed file to `output`.
pub fn burn_subtitles(
    video: &Path,
    srt_text: &str,
    output: &Path,
    final_name: &str,
    on: &dyn Fn(f64),
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> anyhow::Result<()> {
    if srt_text.trim().is_empty() {
        bail!("no subtitle cues to burn");
    }
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let video_abs = abs_path(video);
    let output_abs = abs_path(output);
    let work_dir = output_abs.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(work_dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Use a plain relative ASCII filename in cwd so FFmpeg's libass filter never trips on
    // Windows drive letters (`C:\`) or non-ASCII paths in `-vf subtitles=...`.
    let ascii_name = format!(".aknan-sub-{}-{nanos}.srt", std::process::id());
    let sub_path = work_dir.join(&ascii_name);
    std::fs::write(&sub_path, srt_text.as_bytes())?;
    let _guard = TempSubFile(sub_path);

    let format = format_for(final_name);
    let expected = len_of(&video_abs).max(1);
    let mut cmd = Command::new(&ffmpeg);
    cmd.current_dir(work_dir)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&video_abs)
        .args([
            "-vf",
            &format!("subtitles={ascii_name}"),
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "22",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
        ]);
    if format == "mp4" && expected < FASTSTART_UP_TO {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-f", format]).arg(&output_abs);
    let r = run_watched(&mut cmd, &output_abs, expected, on, cancel);
    let made = std::fs::metadata(&output_abs).map(|m| m.len() > 0).unwrap_or(false);
    match r {
        Ok(()) if made => Ok(()),
        Ok(()) => {
            let _ = std::fs::remove_file(&output_abs);
            bail!("ffmpeg could not burn subtitles: nothing was written")
        }
        Err(e) => {
            let _ = std::fs::remove_file(&output_abs);
            if e.is::<crate::http_download::Cancelled>() {
                return Err(e);
            }
            bail!("ffmpeg could not burn subtitles: {e}")
        }
    }
}
