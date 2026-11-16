use super::{DownloadId, DownloadKind, RequestHeaders, Status, Step};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// Snapshot of a download for the UI.
#[derive(Debug, Clone)]
pub struct DownloadView {
    pub id: DownloadId,
    pub url: String,
    pub file_name: String,
    pub save_path: PathBuf,
    pub category: String,
    pub kind: DownloadKind,
    pub status: Status,
    pub error: Option<String>,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    /// bytes per second
    pub speed: f64,
    pub eta_secs: Option<u64>,
    pub active_connections: u32,
    /// torrents: connected people that have the whole file
    pub seeders: u32,
    pub resumable: bool,
    pub parts_total: u32,
    pub parts_done: u32,
    /// (start, pos, end, active) of every segment, for the connections view
    pub segments: Vec<(u64, u64, u64, bool)>,
    pub queue: Option<u64>,
    pub finished_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
    pub group: Option<u64>,
    /// the checksum the finished file must have ("sha256:...")
    pub checksum: Option<String>,
    pub speed_limit_kb: Option<u64>,
    pub on_complete: crate::config::AfterDone,
    pub added_at: DateTime<Utc>,
    pub last_try: Option<DateTime<Utc>>,
    pub headers: RequestHeaders,
    /// page the download came from
    pub parent_page: Option<String>,
    pub queue_pos: u64,
    /// the connection dropped: waiting for the internet to come back
    pub waiting: bool,
    /// after the data: what it does now and how far (0..1)
    pub step: Option<(Step, f32)>,
    /// video pages: the quality chosen ("1080p|MP4 AV1")
    pub source_key: Option<String>,
}

impl DownloadView {
    pub fn referer(&self) -> &str {
        self.headers.referer.as_deref().unwrap_or("")
    }
    pub fn progress(&self) -> f32 {
        if self.status == Status::Completed {
            return 1.0;
        }
        match self.total_size {
            Some(t) if t > 0 => (self.downloaded as f64 / t as f64).min(1.0) as f32,
            _ if self.parts_total > 0 => self.parts_done as f32 / self.parts_total as f32,
            _ => 0.0,
        }
    }
}

/// Human readable size: 1.5 MB
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}
