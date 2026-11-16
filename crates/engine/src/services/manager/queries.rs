use super::{Event, Manager};
use crate::sync::LockOk;
use crate::config::AfterDone;
use crate::model::{DownloadId, Status};
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};

impl Manager {
    /// Sounds, messages: what happened since the last call.
    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.inner.lock_ok().events)
    }

    /// Shut down / sleep / hibernate asked for (the app shows a countdown first).
    /// A new number for downloads added together (a playlist, a batch).
    pub fn new_group(&self) -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = Utc::now().timestamp_millis().max(1) as u64;
        base.wrapping_mul(1000) + NEXT.fetch_add(1, Ordering::Relaxed) % 1000
    }

    /// How the downloads added together with others are doing: (all,
    /// finished, failed, still running or waiting).
    pub fn group_state(&self, group: u64) -> (usize, usize, usize, bool) {
        let inner = self.inner.lock_ok();
        let mut s = (0, 0, 0, false);
        for e in inner.entries.values().filter(|e| e.rec.group == Some(group)) {
            s.0 += 1;
            match e.rec.status {
                Status::Completed => s.1 += 1,
                Status::Failed => s.2 += 1,
                _ => {}
            }
            if e.task.is_some() || e.rec.status == Status::Queued {
                s.3 = true;
            }
        }
        s
    }

    /// A download still putting its file together (joining its parts,
    /// converting, checking it, moving it into its folder): its name. The
    /// computer is not shut down while there is one.
    pub fn finishing(&self) -> Option<String> {
        let inner = self.inner.lock_ok();
        inner
            .entries
            .values()
            .find(|e| e.task.is_some() && (e.progress.assembling.load(Ordering::Relaxed) || e.progress.step.load(Ordering::Relaxed) != 0))
            .map(|e| e.rec.file_name.clone())
    }

    pub fn take_power_request(&self) -> Option<AfterDone> {
        self.inner.lock_ok().power_request.take()
    }

    /// Every address in the list (for "Export").
    pub fn all_urls(&self) -> Vec<String> {
        let inner = self.inner.lock_ok();
        inner
            .entries
            .values()
            .map(|e| e.rec.source_page.clone().unwrap_or_else(|| e.rec.url.clone()))
            .collect()
    }

    /// Is this address already in the list? Returns its id.
    pub fn find_url(&self, url: &str) -> Option<DownloadId> {
        let inner = self.inner.lock_ok();
        inner
            .entries
            .values()
            .find(|e| e.rec.url == url || e.rec.source_page.as_deref() == Some(url))
            .map(|e| e.rec.id)
    }

    pub fn remove_completed(&self) {
        let mut inner = self.inner.lock_ok();
        inner.entries.retain(|_, e| e.rec.status != Status::Completed);
        inner.dirty = true;
    }

    /// Downloads that finished since the last call (for the "complete" popup).
    pub fn take_completed(&self) -> Vec<DownloadId> {
        std::mem::take(&mut self.inner.lock_ok().completed)
    }

    /// The scheduler asked the app to close.
    pub fn take_exit_request(&self) -> bool {
        std::mem::take(&mut self.inner.lock_ok().exit_requested)
    }

    /// Bytes downloaded inside the current quota window (for Options).
    pub fn quota_used(&self) -> u64 {
        self.inner.lock_ok().quota_log.iter().map(|(_, b)| *b).sum()
    }

}
