use serde::Serialize;

/// One choice in the "Download this video" menu.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YtOption {
    /// Stable id of this choice ("1080p60|MP4"), used to get fresh links later
    pub key: String,
    pub label: String,
    pub sub: String,
    pub quality: String,
    pub video_url: String,
    pub audio_url: Option<String>,
    pub ext: String,
    pub size: u64,
    /// headers these links need (mostly the User-Agent and Referer)
    pub headers: Vec<(String, String)>,
    /// plain words for the resolution: "SD", "HD", "Full HD", "2K", "4K", "8K"
    pub tag: String,
    /// "H.264", "VP9", "AV1" ("" = not known / audio)
    pub codec: String,
    /// length of the video in seconds (0 = unknown)
    pub duration: u64,
    /// bitrate in kbit/s (0 = unknown)
    pub kbps: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YtInfo {
    pub title: String,
    /// e.g. "YouTube", "Instagram", "TikTok"
    pub site: String,
    pub options: Vec<YtOption>,
    #[serde(default)]
    pub subtitles: Vec<super::sub_tracks::YtSubtitle>,
}

/// The "MP3" choice: the sound is downloaded, then turned into an MP3 file.
pub const MP3_KEY: &str = "audio|MP3";
/// bitrate of the MP3 files we make
pub const MP3_KBPS: u64 = 192;

/// The MP3 quality chosen in Options (kbps): 128, 192, 256 or 320.
static MP3_RATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(MP3_KBPS);

pub fn set_mp3_kbps(kbps: u32) {
    let k = match kbps {
        0..=159 => 128,
        160..=223 => 192,
        224..=287 => 256,
        _ => 320,
    };
    MP3_RATE.store(k, std::sync::atomic::Ordering::Relaxed);
}

pub fn mp3_kbps() -> u64 {
    MP3_RATE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Plain words for a resolution, so the choice is easy to understand.
pub fn quality_tag(height: u64) -> &'static str {
    match height {
        0 => "",
        h if h >= 4320 => "8K",
        h if h >= 2160 => "4K",
        h if h >= 1440 => "2K",
        h if h >= 1080 => "Full HD",
        h if h >= 720 => "HD",
        _ => "SD",
    }
}

pub(super) fn with_tag(q: &str, height: u64) -> String {
    match quality_tag(height) {
        "" => q.to_string(),
        t => format!("{q} {t}"),
    }
}

/// A post's title without the counts the site puts before it
/// ("995K views · 9.9K reactions | The real title" -> "The real title").
pub(super) fn post_title(title: &str) -> String {
    let t = title.trim();
    if let Some((head, rest)) = t.split_once(" | ") {
        let counts = head.split('·').all(|part| {
            let p = part.trim();
            let starts_with_number = p.chars().next().is_some_and(|c| c.is_ascii_digit() || ('٠'..='٩').contains(&c));
            starts_with_number && p.split_whitespace().count() <= 3
        });
        if counts && !rest.trim().is_empty() {
            return rest.trim().to_string();
        }
    }
    t.to_string()
}
