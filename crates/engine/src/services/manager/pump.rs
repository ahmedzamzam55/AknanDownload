use super::Manager;
use crate::sync::{LockOk, RwOk};
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Status};
use crate::segments;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// After the error of a queue file that is tried again.
pub(super) const TRYING_AGAIN: &str = " (trying again)";
/// How long a failed queue file waits before it is tried again.
#[cfg(not(test))]
pub(super) const RETRY_WAIT: Duration = Duration::from_secs(20);
#[cfg(test)]
pub(super) const RETRY_WAIT: Duration = Duration::from_millis(300);

impl Manager {
    /// Launch waiting downloads while there are free slots: no global limit
    /// unless set in options, and each queue runs its own number of files at
    /// a time (1 = one after another).
    pub(super) fn pump(self: &Arc<Self>) {
        let settings = self.settings.read_ok().clone();
        let mut launch = Vec::new();
        {
            let mut inner = self.inner.lock_ok();
            if inner.quota_blocked {
                return;
            }
            let mut running = inner.entries.values().filter(|e| e.task.is_some()).count();
            let mut per_queue: HashMap<u64, usize> = HashMap::new();
            for e in inner.entries.values().filter(|e| e.task.is_some()) {
                if let Some(q) = e.rec.queue {
                    *per_queue.entry(q).or_default() += 1;
                }
            }
            // queue files go in their queue order
            let mut order: Vec<(u64, DownloadId)> = inner
                .entries
                .values()
                .filter(|e| e.rec.status == Status::Queued && e.task.is_none())
                .map(|e| (if e.rec.queue.is_some() { e.rec.queue_pos } else { e.rec.id }, e.rec.id))
                .collect();
            order.sort();
            let mut waiting = false;
            for (_, id) in order {
                let Some(e) = inner.entries.get_mut(&id) else { continue };
                // tried again by the queue: not at once (the same trouble,
                // a site that wants a pause), a little later
                if e.rec.retry_count > 0
                    && e.rec.error.as_deref().is_some_and(|x| x.ends_with(TRYING_AGAIN))
                    && e.rec.last_try.is_some_and(|t| Utc::now().signed_duration_since(t) < chrono::Duration::from_std(RETRY_WAIT).unwrap_or_default())
                {
                    waiting = true;
                    continue;
                }
                // its data is all here (only the joining / moving is left,
                // after the program was closed): it does not wait for a free place
                let ready = {
                    let segs = e.segs.lock_ok();
                    let audio = e.audio_segs.lock_ok();
                    e.rec.kind != DownloadKind::Torrent
                        && e.rec.clip.is_none()
                        && !segs.is_empty()
                        && segments::all_finished(&segs)
                        && (e.rec.kind != DownloadKind::Dash || (!audio.is_empty() && segments::all_finished(&audio)))
                };
                if !ready && settings.max_active > 0 && running >= settings.max_active as usize {
                    continue;
                }
                if e.rec.status != Status::Queued || e.task.is_some() {
                    continue;
                }
                if let Some(q) = e.rec.queue {
                    let limit = settings.queue(q).map(|d| d.max_parallel).unwrap_or(1);
                    let now = per_queue.get(&q).copied().unwrap_or(0);
                    if limit > 0 && now >= limit as usize {
                        continue;
                    }
                    *per_queue.entry(q).or_default() += 1;
                }
                let token = CancellationToken::new();
                e.task = Some(token.clone());
                e.rec.status = Status::Connecting;
                e.rec.last_try = Some(Utc::now());
                e.progress.assembling.store(false, Ordering::Relaxed);
                launch.push((id, token));
                running += 1;
            }
            inner.retry_waiting = waiting;
        }
        for (id, token) in launch {
            let m = self.clone();
            self.rt.spawn(async move { m.run_entry(id, token).await });
        }
    }

    /// It can go on from where it stopped (a later try keeps what it has).
    pub(super) fn resumable(&self, id: DownloadId) -> bool {
        let inner = self.inner.lock_ok();
        inner.entries.get(&id).map(|e| e.rec.resumable || e.rec.kind != DownloadKind::Http || e.rec.clip.is_some()).unwrap_or(false)
    }

    pub(super) fn update(&self, id: DownloadId, f: impl FnOnce(&mut DownloadRecord)) {
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            f(&mut e.rec);
        }
        inner.dirty = true;
    }
}
