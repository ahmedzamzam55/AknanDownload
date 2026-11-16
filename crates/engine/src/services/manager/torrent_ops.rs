use super::{Manager, TorrentLive};
use crate::sync::{LockOk, RwOk};
use crate::model::{DownloadId, DownloadKind};
use crate::torrent;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// How long people may be connected to a torrent without sending anything
/// before others are looked for.
pub(super) const TORRENT_DRY: Duration = Duration::from_secs(120);
/// What the user sees meanwhile.
pub const TORRENT_DRY_TEXT: &str = "connected to people who send nothing yet: looking for others who have the file";

impl Manager {
    /// The "Download File Info" window was confirmed for a download that
    /// already started in the background: give it the final folder, name,
    /// note and login. Works while it downloads.
    /// What a running torrent's window shows.
    pub fn torrent_live(&self, id: DownloadId) -> Option<TorrentLive> {
        self.torrent_live.lock_ok().get(&id).cloned()
    }

    /// The torrent's files to get ("1,3"; None: all), while it runs too.
    pub fn set_torrent_files(&self, id: DownloadId, select: Option<String>) {
        self.update(id, move |r| r.torrent_select = select.filter(|s| !s.trim().is_empty()));
    }

    /// A tracker the user adds to a torrent (refused: not a tracker address).
    pub fn add_torrent_tracker(&self, id: DownloadId, tracker: &str) -> bool {
        let t = tracker.trim().to_string();
        if !torrent::is_tracker(&t) {
            return false;
        }
        self.update(id, move |r| {
            if !r.torrent_trackers.contains(&t) && r.torrent_trackers.len() < 100 {
                r.torrent_trackers.push(t);
            }
        });
        true
    }

    /// "First and last pieces first" of this torrent.
    pub fn torrent_first_last(&self, id: DownloadId) -> bool {
        self.inner.lock_ok().entries.get(&id).map(|e| e.rec.torrent_first_last).unwrap_or(false)
    }

    /// The first and last pieces of each file first (a video plays sooner).
    pub fn set_torrent_first_last(&self, id: DownloadId, on: bool) {
        self.update(id, move |r| r.torrent_first_last = on);
    }

    /// Check every piece on the disk again (stopped, checked, then on).
    pub fn recheck_torrent(self: &Arc<Self>, id: DownloadId) {
        self.update(id, |r| r.torrent_recheck = true);
        self.pause(id);
        self.start(id);
    }

    /// The torrent's magnet link (to give to someone).
    pub fn magnet_link(&self, id: DownloadId) -> Option<String> {
        let url = {
            let inner = self.inner.lock_ok();
            inner.entries.get(&id).filter(|e| e.rec.kind == DownloadKind::Torrent).map(|e| e.rec.url.clone())?
        };
        if torrent::is_magnet(&url) {
            return Some(url);
        }
        torrent::magnet_of(&std::fs::read(&url).ok()?)
    }

    /// The checksum the finished file must have (None: none of the user's).
    pub fn set_checksum(&self, id: DownloadId, checksum: Option<String>) {
        self.update(id, move |r| {
            if checksum.is_some() {
                r.checksum = checksum;
            }
        });
    }

    /// (it changes with "only while I use the computer") and the upload one.
    pub(super) async fn torrent_limits(&self) {
        let up = self.settings.read_ok().torrent_upload_kb.saturating_mul(1024);
        let down = self.limit_now.load(Ordering::Relaxed);
        self.torrents.set_limits(if down == u64::MAX { 0 } else { down }, up).await;
    }

    /// A torrent: librqbit gets it, we watch and show its progress.

    /// the people who share it; `then` gets the file.
    pub fn magnet_files(self: &Arc<Self>, magnet: String, then: impl FnOnce(Result<PathBuf, String>) + Send + 'static) {
        let me = self.clone();
        self.rt.spawn(async move {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let dir = me.data_dir.join("torrents").join(format!("list-{n}"));
            let r = me.torrents.metadata(&magnet, &dir, Duration::from_secs(90)).await;
            // the .torrent goes next to the folder (the folder goes)
            let r = r.and_then(|p| {
                let kept = me.data_dir.join("torrents").join(format!("list-{n}.torrent"));
                std::fs::rename(&p, &kept)?;
                Ok(kept)
            });
            let _ = std::fs::remove_dir_all(&dir);
            then(r.map_err(|e| format!("{e:#}")));
        });
    }
}
