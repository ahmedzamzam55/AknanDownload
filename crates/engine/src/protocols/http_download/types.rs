use crate::model::RequestHeaders;
use crate::segments::Segment;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long we wait for the next chunk before treating the connection as dead.
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the other connections are watched before a refused one (403)
/// is taken for "one connection too many".
#[cfg(not(test))]
pub(super) const OTHERS_WATCH: Duration = Duration::from_secs(2);
#[cfg(test)]
pub(super) const OTHERS_WATCH: Duration = Duration::from_millis(400);

/// How long a request waits for the server's answer before it is made again.
pub(super) const HEADERS_TIMEOUT: Duration = Duration::from_secs(20);

/// A connection much slower than the others is dropped and made again (the
/// server often gives a new one full speed): checked over this long...
pub(super) const SLOW_WINDOW: Duration = Duration::from_secs(8);
/// ...when it brings less than this...
pub(super) const SLOW_BYTES_PER_SEC: u64 = 48 * 1024;
/// ...and less than this part of what one connection brings on average.
pub(super) const SLOW_SHARE: u64 = 6;

/// This connection is more than the user wants now: it stops (its part is
/// left to the others).
#[derive(Debug)]
pub(super) struct Leave;
impl std::fmt::Display for Leave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection not needed")
    }
}
impl std::error::Error for Leave {}

/// How many connections run and how many were asked to stop.
#[derive(Default)]
pub(super) struct Crew {
    pub(super) running: u32,
    pub(super) leaving: u32,
    /// connections the server dropped lately (when, and the bytes of the
    /// whole download then)
    pub(super) drops: std::collections::VecDeque<(std::time::Instant, u64)>,
    /// the best a connection brought on average (bytes a second): the last
    /// one left alone is held to it
    pub(super) best: u64,
}

/// At the end of a download, parts are shared down to this size (the last
/// piece is not left to one slow connection).
pub(super) const TAIL_SPLIT: u64 = 64 * 1024;

/// After this long without the server refusing (seconds), one connection more
/// is tried again (up to as many as the download began with).
pub(super) const CLIMB_AFTER: u64 = 60;

#[derive(Clone)]
pub struct HttpJob {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub part_path: PathBuf,
    pub connections: u32,
    pub min_split: u64,
    pub retries: u32,
    /// Bytes from earlier stages of the same download (Dash: the video part
    /// when this job is the audio part), added to the progress counter.
    pub progress_base: u64,
    /// Ask for at most this many bytes per HTTP request. YouTube slows a
    /// connection down to playback speed when one request asks for too much,
    /// so for its servers we fetch each part in 10 MB requests.
    pub max_request: Option<u64>,
    /// this download's own speed limit (shared with its other parts)
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
    /// what makes sure every part is of the same file
    pub check: FileCheck,
}

/// What makes sure every part written is of the same file.
#[derive(Clone, Default)]
pub struct FileCheck {
    /// the server's name for this version of the file (a strong ETag): sent
    /// with every part ("If-Range"), so a file changed on the server comes
    /// back whole instead of as a part of the new one
    pub version: Arc<Mutex<Option<String>>>,
    /// the first answer, kept open since the look: the first part is read
    /// from it (a link that works for one request only)
    pub opening: Arc<Mutex<Option<reqwest::Response>>>,
}

impl FileCheck {
    pub fn new(version: Option<String>, opening: Option<reqwest::Response>) -> FileCheck {
        FileCheck { version: Arc::new(Mutex::new(version)), opening: Arc::new(Mutex::new(opening)) }
    }
}

/// A connection that goes on from where it was re-reads this many bytes
/// before it and compares them with the disk: a file changed on the server
/// (a server that names no version) is caught instead of mixed in.
pub(super) const OVERLAP: u64 = 64;

/// Asked again at once (not a failure of the connection).
#[derive(Debug)]
pub(super) struct Again(pub(super) &'static str);
impl std::fmt::Display for Again {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Again {}

/// The file on the server is not the one the download began with.
pub const FILE_CHANGED: &str = "the file on the server changed since the download began - start the download again";

/// The file changed on the server: nothing more is asked (another try gets
/// the same other file).
#[derive(Debug)]
pub struct FileChanged;
impl std::fmt::Display for FileChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(FILE_CHANGED)
    }
}
impl std::error::Error for FileChanged {}

/// Request size limit for servers known to throttle big range requests.
pub fn max_request_for(url: &str) -> Option<u64> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host.ends_with("googlevideo.com") {
        Some(10 * 1024 * 1024)
    } else {
        None
    }
}

pub type SegTable = Arc<Mutex<Vec<Segment>>>;

#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}
impl std::error::Error for Cancelled {}

/// "bytes 100-199/1000" -> (100, Some(1000)); "bytes 100-199/*" -> (100, None)
pub(super) fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    let total = total.trim().parse().ok();
    Some((start, total))
}
