use super::{Entry, Manager};
use super::cleanup::{remove_temp_files, remove_torrent_data};
use crate::sync::LockOk;
use crate::model::{DownloadId, DownloadKind, Status};
use crate::bt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

impl Manager {
    /// Remove from the list. Partial data is always deleted; the finished
    /// file only when `delete_file` is true.
    pub fn remove(self: &Arc<Self>, id: DownloadId, delete_file: bool) {
        let entry = {
            let mut inner = self.inner.lock_ok();
            inner.dirty = true;
            inner.entries.remove(&id)
        };
        let Some(entry) = entry else { return };
        if let Some(t) = &entry.task {
            t.cancel();
        }
        let rec = entry.rec;
        let data_dir = self.data_dir.clone();
        let me = self.clone();
        self.torrent_live.lock_ok().remove(&id);
        let hash = (rec.kind == DownloadKind::Torrent).then(|| bt::hash_of(&rec.url)).flatten();
        self.rt.spawn(async move {
            // a torrent is let go of (it stops sharing, its files are closed)
            if let Some(h) = hash {
                me.torrents.forget(&h).await;
            }
            // give running connections a moment to close their file handles
            tokio::time::sleep(Duration::from_millis(800)).await;
            // (a torrent's last round may have filled it again meanwhile)
            me.torrent_live.lock_ok().remove(&id);
            for _ in 0..5 {
                let mut ok = remove_temp_files(&rec, &me.paths_in_use());
                if delete_file && rec.status == Status::Completed && rec.kind == DownloadKind::Torrent {
                    ok &= remove_torrent_data(&rec);
                } else if delete_file && rec.status == Status::Completed && rec.target_path().exists() {
                    ok &= std::fs::remove_file(rec.target_path()).is_ok();
                }
                // our copy of its .torrent file
                if rec.kind == DownloadKind::Torrent && rec.url.ends_with(".torrent") && Path::new(&rec.url).starts_with(&data_dir) {
                    let _ = std::fs::remove_file(&rec.url);
                }
                if ok {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Remove many downloads from the list at once (files stay on the disk,
    /// unfinished parts are cleaned up): one lock, one background clean-up.
    pub fn remove_many(self: &Arc<Self>, ids: &[DownloadId]) {
        let gone: Vec<Entry> = {
            let mut inner = self.inner.lock_ok();
            inner.dirty = true;
            ids.iter().filter_map(|id| inner.entries.remove(id)).collect()
        };
        if gone.is_empty() {
            return;
        }
        let mut recs = Vec::with_capacity(gone.len());
        let mut hashes = Vec::new();
        for e in gone {
            if let Some(t) = &e.task {
                t.cancel();
            }
            self.torrent_live.lock_ok().remove(&e.rec.id);
            if e.rec.kind == DownloadKind::Torrent {
                hashes.extend(bt::hash_of(&e.rec.url));
            }
            if e.rec.status != Status::Completed {
                recs.push(e.rec);
            }
        }
        if recs.is_empty() && hashes.is_empty() {
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            for h in hashes {
                me.torrents.forget(&h).await;
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            for _ in 0..5 {
                let mut left = Vec::new();
                let in_use = me.paths_in_use();
                for rec in recs {
                    if !remove_temp_files(&rec, &in_use) {
                        left.push(rec);
                    }
                }
                if left.is_empty() {
                    break;
                }
                recs = left;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Take a download back completely: stop it and delete everything it
    /// wrote (parts, temporary files and the finished file), as if it had
    /// never started. Used when "Download File Info" is cancelled.
    pub fn discard(self: &Arc<Self>, id: DownloadId) {
        let entry = {
            let mut inner = self.inner.lock_ok();
            inner.dirty = true;
            inner.entries.remove(&id)
        };
        let Some(entry) = entry else { return };
        if let Some(t) = &entry.task {
            t.cancel();
        }
        let rec = entry.rec;
        let me = self.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            // the file got its name when it was added (a free one), so
            // whatever is there now was written by this download
            for _ in 0..8 {
                let in_use = me.paths_in_use();
                let mut ok = remove_temp_files(&rec, &in_use);
                let target = rec.target_path();
                if target.exists() && !in_use.contains(&target) {
                    ok &= std::fs::remove_file(&target).is_ok();
                }
                if ok {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
}
