//! The download manager: owns every download, runs the queue, the scheduler,
//! speed measurement and persistence. The UI only talks to this type.

use crate::config::{AfterDone, Settings};
use crate::http::{Progress, ProxyConfig, Shared};
use crate::http_download::SegTable;
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadId, DownloadRecord};
use crate::torrent;
use crate::bt;
use crate::system;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

mod add;
mod cleanup;
mod clip_cut;
mod clip_job;
mod control;
mod edit;
mod finish;
mod init;
mod post_http;
mod post_subs;
mod probe_ops;
mod pump;
mod queries;
mod queues;
mod remove;
mod resolve;
mod retarget;
mod run_dash;
mod run_ftp;
mod run_http;
mod run_torrent;
mod runner;
mod scheduler;
mod tick;
mod torrent_ops;
mod views;
#[cfg(test)]
mod tests;

pub use clip_cut::ClipSource;
pub use edit::DetailsEdit;
pub use init::default_data_dir;
pub use torrent_ops::TORRENT_DRY_TEXT;
pub use views::PreviewSource;

pub(super) struct Entry {
    pub(super) rec: DownloadRecord,
    pub(super) segs: SegTable,
    /// Dash downloads only: segment table of the audio stream
    pub(super) audio_segs: SegTable,
    /// this download's own speed limit (on top of the global one)
    pub(super) limiter: Arc<SpeedLimiter>,
    pub(super) progress: Arc<Progress>,
    /// `Some` while a task is working on this download
    pub(super) task: Option<CancellationToken>,
    pub(super) speed: SpeedMeter,
    /// bytes counted for the download quota so far
    pub(super) quota_prev: u64,
}

/// Things that happened, for sounds and messages in the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Completed(DownloadId),
    Failed(DownloadId),
    QueueStarted(u64),
    QueueDone(u64),
    /// Options > Connection > download limit reached; downloads were stopped
    QuotaReached,
    /// the queue wants this finished file opened
    OpenFile(DownloadId),
}

#[derive(Default)]
pub(super) struct SpeedMeter {
    /// (time, bytes) samples of the last ~2 seconds
    pub(super) samples: std::collections::VecDeque<(Instant, u64)>,
    pub(super) value: f64,
    /// the speed over the last ~10 seconds: the time left follows it, so it
    /// stays steady (and still shows when the speed drops for a moment)
    pub(super) steady: f64,
}

pub(super) struct Inner {
    pub(super) entries: BTreeMap<DownloadId, Entry>,
    /// a queue file that failed waits a little before it is tried again
    pub(super) retry_waiting: bool,
    pub(super) next_id: DownloadId,
    pub(super) dirty: bool,
    pub(super) last_save: Instant,
    pub(super) sched_minute: Option<(u32, u32, u32)>,
    pub(super) completed: Vec<DownloadId>,
    pub(super) exit_requested: bool,
    pub(super) when_all_done: AfterDone,
    pub(super) all_done_seen_active: bool,
    pub(super) events: Vec<Event>,
    /// shut down / sleep / hibernate waiting for the countdown window
    pub(super) power_request: Option<AfterDone>,
    /// (time, bytes) of the download quota window
    pub(super) quota_log: VecDeque<(Instant, u64)>,
    /// stopped because of the quota; started again when it allows
    pub(super) quota_paused: Vec<DownloadId>,
    pub(super) quota_blocked: bool,
    /// queues started (by hand or by their schedule) whose "when done" is armed
    pub(super) queue_armed: HashSet<u64>,
}

pub struct Manager {
    pub(super) inner: Mutex<Inner>,
    pub(super) settings: RwLock<Settings>,
    pub(super) shared: Arc<Shared>,
    pub(super) proxy: Mutex<ProxyConfig>,
    pub(super) rt: Handle,
    pub(super) data_dir: PathBuf,
    /// the hidden torrent program (started when the first torrent runs)
    pub(super) torrents: Arc<bt::Torrents>,
    /// downloads whose link stopped working: the next start gets a new link
    /// and goes on only when it is the same file (size, ranges)
    pub(super) relink: Mutex<HashSet<DownloadId>>,
    /// what running torrents show in their window (files, people, pieces)
    pub(super) torrent_live: Mutex<HashMap<DownloadId, TorrentLive>>,
    /// the computer is woken for the next scheduled start (when, the timer)
    pub(super) waker: Mutex<Option<(i64, system::Waker)>>,
    /// the limit the downloads have now (the speed limit, or none while
    /// nobody uses the computer)
    pub(super) limit_now: std::sync::atomic::AtomicU64,
    pub(super) ticks: std::sync::atomic::AtomicU64,
}

/// What a running torrent shows in its window.
#[derive(Debug, Clone, Default)]
pub struct TorrentLive {
    pub files: Vec<torrent::FileState>,
    pub peers: Vec<torrent::Peer>,
    pub trackers: Vec<String>,
    /// which parts of it are here: (from, to, here), 0..1
    pub pieces: Vec<(f32, f32, bool)>,
    pub uploaded: u64,
}

/// A minute without the mouse or the keyboard: nobody uses the computer.
pub(super) const IDLE_AFTER: u64 = 60;

/// The limit for all downloads now: the speed limit of Options, or none
/// while nobody uses the computer (when Options say so).
pub(super) fn limit_for(s: &Settings, idle: Option<u64>) -> u64 {
    if s.limit_only_active && idle.is_some_and(|i| i >= IDLE_AFTER) {
        0
    } else {
        s.effective_limit()
    }
}
