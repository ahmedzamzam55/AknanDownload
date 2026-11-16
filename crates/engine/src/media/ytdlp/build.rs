use super::formats::{as_u64, fmt_size, main_entry, parse_formats, quality_of, Fmt};
use super::options::{mp3_kbps, post_title, quality_tag, with_tag, YtInfo, YtOption, MP3_KEY};
use crate::model::RequestHeaders;
use serde_json::Value;

/// Turn yt-dlp's JSON into download choices: every resolution paired with the
/// best matching sound, plus "audio only".
pub fn build_options(full: &Value) -> YtInfo {
    let info = main_entry(full);
    let title = post_title(info["title"].as_str().or_else(|| full["title"].as_str()).unwrap_or("video"));
    let site = info["extractor_key"]
        .as_str()
        .or_else(|| full["extractor_key"].as_str())
        .unwrap_or("")
        .to_string();
    let formats = parse_formats(info);
    let duration = as_u64(&info["duration"]).unwrap_or(0);

    let mut audios: Vec<&Fmt> = formats
        .iter()
        .filter(|f| f.has_audio() && !f.has_video() && !f.hls)
        .collect();
    // the original language first, then the best quality
    audios.sort_by(|a, b| {
        b.audio_rank
            .cmp(&a.audio_rank)
            .then(b.abr.partial_cmp(&a.abr).unwrap_or(std::cmp::Ordering::Equal))
    });
    let best_audio = |container: &str| -> Option<&Fmt> {
        audios
            .iter()
            .copied()
            .find(|a| if container == "webm" { a.ext == "webm" } else { a.ext == "m4a" || a.ext == "mp4" })
    };

    let mut videos: Vec<&Fmt> = formats.iter().filter(|f| f.has_video()).collect();
    videos.sort_by(|a, b| {
        b.height
            .cmp(&a.height)
            .then(b.fps.cmp(&a.fps))
            .then(a.hls.cmp(&b.hls)) // direct files before streams
            .then(a.codec_rank().cmp(&b.codec_rank()))
            .then(b.size.cmp(&a.size))
    });

    let mut options = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for v in videos {
        let q = quality_of(v);
        let container = if v.ext == "webm" { "webm" } else { "mp4" };
        if v.has_audio() {
            // video and sound in one file (most sites besides YouTube)
            let key = format!("{q}|{}+A", container.to_uppercase());
            if !seen.insert(key.clone()) {
                continue;
            }
            let ext = if v.hls { "ts" } else { container };
            options.push(YtOption {
                label: format!("{} · {}", with_tag(&q, v.height), ext.to_uppercase()),
                sub: fmt_size(v.size),
                key,
                quality: q,
                video_url: v.url.clone(),
                audio_url: None,
                ext: ext.into(),
                size: v.size,
                headers: v.headers.clone(),
                tag: quality_tag(v.height).into(),
                codec: v.codec_name().into(),
                duration,
                kbps: v.tbr.round() as u64,
            });
            continue;
        }
        if v.hls {
            continue; // video-only stream: we can't add sound to it
        }
        // AV1 is its own choice next to H.264: the same picture in a much
        // smaller file (newer players and computers)
        let av1 = v.codec_name() == "AV1" && container == "mp4";
        let key = format!("{q}|{}{}", container.to_uppercase(), if av1 { " AV1" } else { "" });
        if !seen.insert(key.clone()) {
            continue; // already have this resolution in this container (better codec)
        }
        let (audio, ext) = match best_audio(container) {
            Some(a) => (a, container.to_string()),
            None => match audios.first() {
                Some(a) => (*a, "mkv".to_string()),
                None => continue,
            },
        };
        let size = if v.size > 0 && audio.size > 0 { v.size + audio.size } else { 0 };
        options.push(YtOption {
            label: format!("{} · {}{}", with_tag(&q, v.height), ext.to_uppercase(), v.codec_tag()),
            sub: fmt_size(size),
            key,
            quality: q,
            video_url: v.url.clone(),
            audio_url: Some(audio.url.clone()),
            ext,
            size,
            headers: v.headers.clone(),
            tag: quality_tag(v.height).into(),
            codec: v.codec_name().into(),
            duration,
            kbps: (v.tbr + audio.abr).round() as u64,
        });
    }
    if let Some(a) = best_audio("mp4").or_else(|| audios.first().copied()) {
        let ext = if a.ext == "webm" { "webm" } else if a.ext == "mp3" { "mp3" } else { "m4a" };
        let kbps = a.abr.round() as u64;
        options.push(YtOption {
            key: format!("audio|{}", ext.to_uppercase()),
            label: if kbps > 0 {
                format!("Audio only · {} {kbps}k", ext.to_uppercase())
            } else {
                format!("Audio only · {}", ext.to_uppercase())
            },
            sub: fmt_size(a.size),
            quality: "audio".into(),
            video_url: a.url.clone(),
            audio_url: None,
            ext: ext.into(),
            size: a.size,
            headers: a.headers.clone(),
            tag: String::new(),
            codec: String::new(),
            duration,
            kbps,
        });
        if ext != "mp3" {
            // the same sound, turned into an MP3 file after the download
            // (plays on every phone, car radio and old player)
            let rate = mp3_kbps();
            let mp3_size = if duration > 0 { duration * rate * 1000 / 8 } else { 0 };
            options.push(YtOption {
                key: MP3_KEY.into(),
                label: format!("Audio · MP3 {rate}k"),
                sub: fmt_size(mp3_size),
                quality: "mp3".into(),
                video_url: a.url.clone(),
                audio_url: None,
                ext: "mp3".into(),
                size: mp3_size,
                headers: a.headers.clone(),
                tag: String::new(),
                codec: String::new(),
                duration,
                kbps: rate,
            });
        }
    }
    YtInfo { title, site, options }
}

/// Use the headers yt-dlp asked for when downloading the links.
pub fn apply_headers(target: &mut RequestHeaders, headers: &[(String, String)]) {
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if lower == "user-agent" {
            target.user_agent = Some(v.clone());
        } else if lower == "referer" {
            target.referer = Some(v.clone());
        } else if lower == "accept" || lower == "accept-encoding" || lower == "cookie" || lower == "range" {
            // we set these ourselves
        } else {
            target.extra.retain(|(ek, _)| !ek.eq_ignore_ascii_case(k));
            target.extra.push((k.clone(), v.clone()));
        }
    }
}

