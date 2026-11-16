//! "Download a part of a video": ffmpeg cuts the part out, either from a
//! file here (a stream whose parts were fetched) or from our own address
//! of the site's video (clipserve), so only that part is downloaded.
//!
//! Two kinds of cut:
//! * quick (the usual one): nothing is made again, it takes seconds and keeps
//!   the quality as it is, but a video can only start on a whole picture,
//!   so the part may start a second or two before the time asked;
//! * exact: the picture is made again (H.264) and the part starts on the
//!   very second asked. Slower, and it works the processor.
//!
//! Sound alone is always cut on the very second.

use anyhow::bail;
use std::path::Path;
use tokio_util::sync::CancellationToken;

mod probe;
#[cfg(test)]
mod tests;

pub use probe::{look, parse_duration, parse_picture};
use probe::{h264_maker, readable, run_ffmpeg, video_kbps};

/// What the part becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// picture and sound (MP4)
    Video,
    /// MP3 at this many kb/s, with this title
    Mp3(u64, Option<String>),
    /// sound alone in an MP4 file (.m4a)
    M4a,
    /// sound alone in an Ogg file (Opus)
    Ogg,
}

impl Output {
    /// From the file's name (and "Audio · MP3").
    pub fn for_name(name: &str, mp3: Option<(u64, String)>) -> Output {
        if let Some((kbps, title)) = mp3 {
            return Output::Mp3(kbps, Some(title));
        }
        match crate::filename::extension(name).as_str() {
            "mp3" => Output::Mp3(crate::ytdlp::mp3_kbps(), None),
            "m4a" | "aac" => Output::M4a,
            "ogg" | "opus" => Output::Ogg,
            _ => Output::Video,
        }
    }
    fn is_video(&self) -> bool {
        *self == Output::Video
    }
}

/// How the cut went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Done {
    /// as asked
    AsAsked,
    /// the exact cut can't be done for this video (its kind of picture
    /// can't be read here): it was cut the quick way
    QuickInstead,
}

/// What ffmpeg says about the first video of an input.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Picture {
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

/// "Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv), 1280x720 [SAR 1:1 DAR 16:9], ..."
/// The ffmpeg words for one cut (without the program, the inputs' place
/// in `inputs` and the output file).
pub fn cut_args(inputs: &[String], from: f64, len: f64, exact_with: Option<(&str, u64)>, out: &Output) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-nostats", "-progress", "pipe:1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for i in inputs {
        a.extend(["-ss".into(), format!("{from:.3}"), "-i".into(), i.clone()]);
    }
    a.extend(["-t".into(), format!("{len:.3}")]);
    let two = inputs.len() > 1;
    if out.is_video() {
        if two {
            a.extend(["-map", "0:v:0", "-map", "1:a:0?"].map(String::from));
        } else {
            a.extend(["-map", "0:v:0?", "-map", "0:a:0?"].map(String::from));
        }
        match exact_with {
            Some((maker, kbps)) => {
                a.extend(["-c:v".into(), maker.into(), "-b:v".into(), format!("{kbps}k"), "-maxrate".into(), format!("{}k", kbps * 3 / 2)]);
                if maker == "libx264" {
                    a.extend(["-preset", "veryfast"].map(String::from));
                }
                a.extend(["-vf", "format=yuv420p", "-c:a", "aac", "-b:a", "192k"].map(String::from));
            }
            None => a.extend(["-c", "copy", "-avoid_negative_ts", "make_zero"].map(String::from)),
        }
        a.extend(["-movflags", "+faststart", "-f", "mp4"].map(String::from));
    } else {
        let sound = if two { "1:a:0" } else { "0:a:0" };
        a.extend(["-vn".into(), "-map".into(), sound.into(), "-map_metadata".into(), "-1".into()]);
        match out {
            Output::Mp3(kbps, title) => {
                a.extend(["-c:a".into(), "libmp3lame".into(), "-b:a".into(), format!("{kbps}k"), "-id3v2_version".into(), "3".into()]);
                if let Some(t) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    a.extend(["-metadata".into(), format!("title={}", t.chars().take(200).collect::<String>())]);
                }
                a.extend(["-f", "mp3"].map(String::from));
            }
            Output::M4a => a.extend(["-c:a", "copy", "-movflags", "+faststart", "-f", "mp4"].map(String::from)),
            _ => a.extend(["-c:a", "copy", "-f", "ogg"].map(String::from)),
        }
    }
    a
}

/// Cut `len` seconds from `from` of `inputs` (a picture, and maybe its
/// sound apart) into `output`. `done(fraction)` while it works.
#[allow(clippy::too_many_arguments)]
pub async fn cut(
    ffmpeg: &Path,
    inputs: &[String],
    from: f64,
    len: f64,
    exact: bool,
    out: &Output,
    output: &Path,
    done: impl Fn(f64) + Sync,
    cancel: &CancellationToken,
) -> anyhow::Result<Done> {
    if inputs.is_empty() || len <= 0.0 {
        bail!("nothing to cut");
    }
    let mut result = Done::AsAsked;
    let mut exact_with = None;
    if exact && out.is_video() {
        let pic = look(ffmpeg, &inputs[0]).await.ok().and_then(|(_, p)| p);
        match pic {
            Some(p) if readable(&p.codec) => exact_with = Some((h264_maker(ffmpeg), video_kbps(&p))),
            _ => result = Done::QuickInstead,
        }
    }
    let progress = move |secs: f64| done((secs / len).clamp(0.0, 1.0));
    match run_ffmpeg(ffmpeg, cut_args(inputs, from, len, exact_with, out), output, &progress, cancel).await {
        Ok(()) => Ok(result),
        Err(e) if e.is::<crate::http_download::Cancelled>() => Err(e),
        // the picture could not be made again: the quick cut
        Err(_) if exact_with.is_some() => {
            run_ffmpeg(ffmpeg, cut_args(inputs, from, len, None, out), output, &progress, cancel).await?;
            Ok(Done::QuickInstead)
        }
        Err(e) => Err(e),
    }
}
