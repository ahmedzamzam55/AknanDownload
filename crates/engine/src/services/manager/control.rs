use super::Manager;
use super::cleanup::{paths_in_use_but, remove_temp_files};
use super::views::collect_records;
use crate::sync::LockOk;
use crate::config::AfterDone;
use crate::model::{DownloadId, Status};
use crate::store;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl Manager {
    /// Resume / start one download (it waits in the queue if too many run).
    pub fn start(self: &Arc<Self>, id: DownloadId) {
        {
            let mut inner = self.inner.lock_ok();
            if let Some(e) = inner.entries.get_mut(&id) {
                if matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    pub fn pause(&self, id: DownloadId) {
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    pub fn start_all(self: &Arc<Self>) {
        {
            let mut inner = self.inner.lock_ok();
            for e in inner.entries.values_mut() {
                if matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    pub fn pause_all(&self) {
        let mut inner = self.inner.lock_ok();
        for e in inner.entries.values_mut() {
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    /// Speed limit for one download (None = no limit). Takes effect at once.
    pub fn set_download_limit(&self, id: DownloadId, kb: Option<u64>) {
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.speed_limit_kb = kb.filter(|k| *k > 0);
            e.limiter.set_rate(e.rec.speed_limit_kb.unwrap_or(0) * 1024);
        }
        inner.dirty = true;
    }

    /// What to do when this download finishes (shut down, sleep...).
    pub fn set_on_complete(&self, id: DownloadId, action: AfterDone) {
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.on_complete = action;
        }
        inner.dirty = true;
    }

    /// What to do once every running and waiting download has finished.
    pub fn set_when_all_done(&self, action: AfterDone) {
        let mut inner = self.inner.lock_ok();
        inner.when_all_done = action;
        inner.all_done_seen_active = false;
    }

    pub fn when_all_done(&self) -> AfterDone {
        self.inner.lock_ok().when_all_done
    }

    /// Start a failed/finished download again from zero.
    pub fn restart(self: &Arc<Self>, id: DownloadId) {
        {
            let mut inner = self.inner.lock_ok();
            let Some(e) = inner.entries.get_mut(&id) else { return };
            if e.task.is_some() {
                return;
            }
            let rec = e.rec.clone();
            let in_use = paths_in_use_but(&inner, id);
            remove_temp_files(&rec, &in_use);
            let Some(e) = inner.entries.get_mut(&id) else { return };
            e.segs.lock_ok().clear();
            e.audio_segs.lock_ok().clear();
            e.progress.bytes.store(0, Ordering::Relaxed);
            e.progress.parts_done.store(0, Ordering::Relaxed);
            e.rec.downloaded = 0;
            e.rec.parts_done = 0;
            e.rec.status = Status::Queued;
            e.rec.error = None;
            inner.dirty = true;
        }
        self.pump();
    }

    /// Stop everything and write the state to disk (call before exiting).
    pub fn shutdown(&self) {
        let records = {
            let mut inner = self.inner.lock_ok();
            for e in inner.entries.values_mut() {
                if let Some(t) = &e.task {
                    t.cancel();
                }
                if e.rec.status.is_running() || e.rec.status == Status::Queued {
                    e.rec.status = Status::Paused;
                }
            }
            collect_records(&inner)
        };
        let _ = store::save_downloads(&self.data_dir.join("downloads.json"), &records);
    }
}
