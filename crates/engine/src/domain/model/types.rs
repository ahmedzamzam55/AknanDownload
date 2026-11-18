use super::Clip;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type DownloadId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Connecting,
    Downloading,
    Assembling,
    Paused,
    Completed,
    Failed,
}

impl Status {
    pub fn is_running(self) -> bool {
        matches!(self, Status::Connecting | Status::Downloading | Status::Assembling)
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Connecting => "Connecting…",
            Status::Downloading => "Downloading",
            Status::Assembling => "Assembling…",
            Status::Paused => "Paused",
            Status::Completed => "Complete",
            Status::Failed => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadKind {
    /// Normal file over HTTP/HTTPS (segmented when the server allows ranges)
    Http,
    /// HLS stream (.m3u8): many small parts joined into one video file
    Hls,
    /// Separate video and audio streams (YouTube style, "DASH"):
    /// both are downloaded, then merged into one file with ffmpeg
    Dash,
    /// a .torrent file or a magnet link, shared with other computers
    Torrent,
}

/// What a download does after the data is all here (its second bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// the picture and the sound into one file
    Join,
    /// a stream into an MP4 file
    ToMp4,
    /// the sound into an MP3 file
    ToMp3,
    /// the file to its folder on another disk
    Move,
    /// the part of a video cut out
    Cut,
    /// the finished file compared with its checksum
    Check,
    /// subtitles rendered directly onto the video frames
    BurnSubs,
}

impl Step {
    pub fn from_code(c: u8) -> Option<Step> {
        Some(match c {
            1 => Step::Join,
            2 => Step::ToMp4,
            3 => Step::ToMp3,
            4 => Step::Move,
            5 => Step::Cut,
            6 => Step::Check,
            7 => Step::BurnSubs,
            _ => return None,
        })
    }
    pub fn code(self) -> u8 {
        match self {
            Step::Join => 1,
            Step::ToMp4 => 2,
            Step::ToMp3 => 3,
            Step::Move => 4,
            Step::Cut => 5,
            Step::Check => 6,
            Step::BurnSubs => 7,
        }
    }
}

/// Subtitle attachment or burn-in requested alongside a video download.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTask {
    pub lang: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub burn: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, with = "crate::secret::opt", skip_serializing_if = "Option::is_none")]
    pub cookies: Option<String>,
}

/// Extra request data, usually handed over by the browser extension so that
/// downloads behind a login work exactly like in the browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestHeaders {
    pub referer: Option<String>,
    /// the site's login cookies: encrypted on the disk (secret.rs)
    #[serde(default, with = "crate::secret::opt")]
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    /// user name + password for the server ("Use authorization")
    /// encrypted on the disk (secret.rs)
    #[serde(default, with = "crate::secret::pair")]
    pub auth: Option<(String, String)>,
    /// a form's download: what the form sends (base64), asked with POST;
    /// encrypted on the disk like cookies
    #[serde(default, with = "crate::secret::opt", skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    /// the form's kind of data ("application/x-www-form-urlencoded"...)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_type: Option<String>,
}

/// What we learn about a URL before downloading it.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub file_name: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub kind: DownloadKind,
    /// the server's "Last-Modified" date
    pub last_modified: Option<DateTime<Utc>>,
    /// the server's name for this version of the file ("ETag")
    pub etag: Option<String>,
    /// the server refused the request as it was (HTTP 403 and the like) and
    /// took it with these headers instead: the download uses them too
    pub headers_used: Option<RequestHeaders>,
    /// the checksum the server sent with the file ("sha256:..."), if any
    pub digest: Option<String>,
}

impl ProbeResult {
    /// The address opens a web page, not a file.
    pub fn is_web_page(&self) -> bool {
        let ct = self.content_type.as_deref().unwrap_or("").to_ascii_lowercase();
        let ext = crate::filename::extension(&self.file_name);
        ct.starts_with("text/html") && !matches!(ext.as_str(), "htm" | "html" | "mht" | "mhtml")
    }
}

/// A request to add a download.
#[derive(Debug, Clone, Default)]
pub struct NewDownload {
    pub url: String,
    pub file_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub headers: RequestHeaders,
    pub connections: Option<u32>,
    /// Result of an earlier probe (from the add dialog), if any
    pub probe: Option<ProbeResult>,
    /// Separate audio stream to merge with `url` (YouTube style downloads)
    pub audio_url: Option<String>,
    /// YouTube page + chosen quality, to get fresh links when old ones expire
    pub source_page: Option<String>,
    pub source_key: Option<String>,
    /// put it in this download queue
    pub queue: Option<u64>,
    /// added together with others (a playlist, a batch): one "finished"
    /// window for all of them
    pub group: Option<u64>,
    /// the user's note from the download dialog
    pub description: Option<String>,
    /// a video page (playlist item): find its links when it starts, in this
    /// quality ("best", "audio", or a height like "720")
    pub pick_quality: Option<String>,
    /// started at once while the "Download File Info" window is open
    /// (its name and folder can change until the user confirms)
    pub early: bool,
    pub start: bool,
    /// torrents: the files to get, 1-based ("1,3,4"); None = all
    pub torrent_select: Option<String>,
    /// only this part of the video / sound
    pub clip: Option<Clip>,
    /// the checksum the finished file must have (what the user pasted)
    pub checksum: Option<String>,
    /// subtitle track to save alongside or burn into a downloaded video
    pub subtitle: Option<SubtitleTask>,
    /// target format ("srt", "vtt", "txt") when downloading a standalone subtitle file
    pub sub_convert: Option<String>,
    pub sub_body: Option<String>,
}
