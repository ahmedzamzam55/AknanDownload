use super::formats::Fmt;

/// Pick the best audio track to mux with a video-only HLS stream (e.g. YouTube Kids / iOS `m3u8_native`).
pub(super) fn pick_split_audio<'a>(v: &Fmt, container: &str, audios: &[&'a Fmt]) -> Option<(&'a Fmt, String)> {
    let same = audios.iter().copied().find(|a| {
        if container == "webm" {
            a.ext == "webm"
        } else {
            a.ext == "m4a" || a.ext == "mp4" || a.hls
        }
    });
    match same {
        Some(a) => Some((a, container.to_string())),
        None => {
            let any = *audios.first()?;
            let ext = if v.hls && any.ext == "webm" { "mkv" } else if v.hls { "mp4" } else { "mkv" };
            Some((any, ext.to_string()))
        }
    }
}

/// Estimated byte size when explicit `filesize` is absent (common on `m3u8_native` splits).
pub(super) fn split_size(v: &Fmt, a: &Fmt, duration: u64) -> u64 {
    if v.size > 0 && a.size > 0 {
        return v.size + a.size;
    }
    if duration > 0 && (v.tbr > 0.0 || a.abr > 0.0) {
        return ((duration as f64) * (v.tbr + a.abr) * 1000.0 / 8.0) as u64;
    }
    0
}
