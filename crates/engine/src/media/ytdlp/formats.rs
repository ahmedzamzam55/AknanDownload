use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct Fmt {
    pub(super) url: String,
    pub(super) ext: String,
    pub(super) vcodec: String,
    pub(super) acodec: String,
    pub(super) height: u64,
    pub(super) fps: u64,
    pub(super) size: u64,
    pub(super) abr: f64,
    pub(super) tbr: f64,
    pub(super) note: String,
    pub(super) hls: bool,
    pub(super) headers: Vec<(String, String)>,
    /// sound track ranking: the video's own language first, translated
    /// ("dubbed", often made automatically) tracks last
    pub(super) audio_rank: i64,
}

impl Fmt {
    /// "unknown" codecs (common outside YouTube) count as present
    pub(super) fn has_video(&self) -> bool {
        self.vcodec != "none"
    }
    pub(super) fn has_audio(&self) -> bool {
        self.acodec != "none"
    }
    /// lower is better: H.264 plays everywhere, then VP9, then AV1
    pub(super) fn codec_rank(&self) -> u8 {
        let c = self.vcodec.to_ascii_lowercase();
        if c.starts_with("avc") || c.starts_with("h264") || c == "unknown" {
            0
        } else if c.starts_with("vp9") || c.starts_with("vp09") {
            1
        } else {
            2
        }
    }
    pub(super) fn codec_name(&self) -> &'static str {
        let c = self.vcodec.to_ascii_lowercase();
        if c.starts_with("av01") {
            "AV1"
        } else if c.starts_with("vp9") || c.starts_with("vp09") {
            "VP9"
        } else if c.starts_with("avc") || c.starts_with("h264") {
            "H.264"
        } else if c.starts_with("hev") || c.starts_with("hvc") || c.starts_with("h265") {
            "H.265"
        } else {
            ""
        }
    }
    pub(super) fn codec_tag(&self) -> &'static str {
        if self.vcodec.to_ascii_lowercase().starts_with("av01") {
            " AV1"
        } else {
            ""
        }
    }
}

pub(super) fn as_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64))
}

fn codec(v: &Value) -> String {
    match v.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "unknown".into(),
    }
}

fn headers_of(f: &Value) -> Vec<(String, String)> {
    f["http_headers"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_one(f: &Value) -> Option<Fmt> {
    let protocol = f["protocol"].as_str().unwrap_or("https");
    let hls = protocol.starts_with("m3u8");
    if !(protocol == "https" || protocol == "http" || hls) {
        return None; // dash manifests, storyboards, rtmp, live fragments
    }
    if f["has_drm"].as_bool() == Some(true) {
        return None;
    }
    let ext = f["ext"].as_str().unwrap_or("mp4").to_ascii_lowercase();
    if ext == "mhtml" {
        return None;
    }
    Some(Fmt {
        url: f["url"].as_str()?.to_string(),
        ext,
        vcodec: codec(&f["vcodec"]),
        acodec: codec(&f["acodec"]),
        height: as_u64(&f["height"]).unwrap_or(0),
        fps: as_u64(&f["fps"]).unwrap_or(0),
        size: as_u64(&f["filesize"]).or_else(|| as_u64(&f["filesize_approx"])).unwrap_or(0),
        abr: f["abr"].as_f64().or_else(|| f["tbr"].as_f64()).unwrap_or(0.0),
        tbr: f["tbr"].as_f64().or_else(|| f["vbr"].as_f64()).unwrap_or(0.0),
        note: f["format_note"].as_str().unwrap_or("").to_string(),
        hls,
        headers: headers_of(f),
        audio_rank: audio_rank(f),
    })
}

/// Videos can carry several sound tracks in different languages; yt-dlp marks
/// the original one (language_preference, "original" / "default" in the
/// note) and translated ones ("dubbed"). Higher = better.
fn audio_rank(f: &Value) -> i64 {
    let note = format!(
        "{} {}",
        f["format_note"].as_str().unwrap_or(""),
        f["format_id"].as_str().unwrap_or("")
    )
    .to_ascii_lowercase();
    let mut rank = f["language_preference"].as_i64().unwrap_or(0);
    if note.contains("original") {
        rank += 100;
    } else if note.contains("default") {
        rank += 50;
    }
    if note.contains("dubbed") || note.contains("descriptive") {
        rank -= 100;
    }
    rank
}

pub(super) fn parse_formats(info: &Value) -> Vec<Fmt> {
    match info["formats"].as_array() {
        Some(list) => list.iter().filter_map(parse_one).collect(),
        // some sites give a single file with no "formats" list
        None => parse_one(info).into_iter().collect(),
    }
}

/// Posts with several videos (playlists, carousels): use the first video.
pub(super) fn main_entry(info: &Value) -> &Value {
    if info["_type"].as_str() == Some("playlist") {
        if let Some(entry) = info["entries"]
            .as_array()
            .and_then(|e| e.iter().find(|x| x["formats"].is_array() || x["url"].is_string()))
        {
            return entry;
        }
    }
    info
}

pub(super) fn fmt_size(n: u64) -> String {
    if n == 0 {
        return "size unknown".into();
    }
    crate::model::fmt_size(n)
}

pub(super) fn quality_of(f: &Fmt) -> String {
    let mut q = if f.height > 0 {
        format!("{}p", f.height)
    } else if !f.note.is_empty() && f.note.len() <= 12 {
        f.note.to_ascii_uppercase()
    } else {
        "Video".into()
    };
    if f.fps > 30 {
        q.push_str(&f.fps.to_string());
    }
    q
}
