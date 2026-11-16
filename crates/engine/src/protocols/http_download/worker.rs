use super::learned::lower_wanted;
use super::types::{Crew, HttpJob, Leave, SegTable, OTHERS_WATCH, TAIL_SPLIT};
use crate::http::{Progress, Shared};
use crate::segments;
use crate::sync::LockOk;
use std::fs::File;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) struct Worker {
    pub(super) shared: Arc<Shared>,
    pub(super) job: HttpJob,
    pub(super) segs: SegTable,
    pub(super) progress: Arc<Progress>,
    pub(super) file: Arc<File>,
    pub(super) cancel: CancellationToken,
    pub(super) segmented: bool,
    pub(super) crew: Arc<Mutex<Crew>>,
    pub(super) wanted: Arc<dyn Fn() -> u32 + Send + Sync>,
    /// the server's "Retry-After" of its last "too many requests" (seconds)
    pub(super) retry_after: std::sync::atomic::AtomicU64,
}

/// Counts the connection out when it ends, however it ends.
pub(super) struct OutOfCrew(Arc<Mutex<Crew>>, bool);
impl Drop for OutOfCrew {
    fn drop(&mut self) {
        let mut c = self.0.lock_ok();
        c.running = c.running.saturating_sub(1);
        if self.1 {
            c.leaving = c.leaving.saturating_sub(1);
        }
    }
}

impl Worker {
    /// A connection was dropped: when that happens again and again (6 in a
    /// minute) while the download as a whole goes on, the server allows
    /// fewer connections: half as many are wanted. True when that was decided.
    pub(super) fn server_drops_too_many(&self) -> bool {
        let now = std::time::Instant::now();
        let bytes = self.progress.bytes.load(Ordering::Relaxed);
        let mut c = self.crew.lock_ok();
        c.drops.push_back((now, bytes));
        while c.drops.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(60)) {
            c.drops.pop_front();
        }
        let running = c.running.saturating_sub(c.leaving);
        let first_bytes = c.drops.front().map(|(_, b)| *b).unwrap_or(bytes);
        // the others still bring data (not the internet that is gone)
        let going_on = bytes.saturating_sub(first_bytes) >= 1 << 20;
        if c.drops.len() >= 6 && going_on && running > 2 {
            c.drops.clear();
            drop(c);
            lower_wanted(&self.progress, self.job.connections.max(1), (running / 2).max(2));
            true
        } else {
            false
        }
    }

    /// More connections run than wanted now: this one stops (counted at once,
    /// so only as many stop as needed).
    /// The other connections still bring data (watched for 2 seconds): the
    /// link works, this connection alone was refused.
    pub(super) async fn others_bring_data(&self) -> bool {
        let others = {
            let c = self.crew.lock_ok();
            c.running.saturating_sub(c.leaving)
        };
        if others < 2 {
            return false;
        }
        let before = self.progress.bytes.load(Ordering::Relaxed);
        tokio::select! {
            _ = tokio::time::sleep(OTHERS_WATCH) => {}
            _ = self.cancel.cancelled() => return false,
        }
        self.progress.bytes.load(Ordering::Relaxed) > before
    }

    pub(super) fn should_leave(&self) -> bool {
        let want = (self.wanted)();
        let mut c = self.crew.lock_ok();
        if c.running.saturating_sub(c.leaving) > want {
            c.leaving += 1;
            true
        } else {
            false
        }
    }

    pub(super) async fn run(self) -> anyhow::Result<()> {
        let mut out = OutOfCrew(self.crew.clone(), false);
        let r = self.work().await;
        match r {
            Err(e) if e.is::<Leave>() => {
                out.1 = true;
                Ok(())
            }
            other => other,
        }
    }

    async fn work(&self) -> anyhow::Result<()> {
        loop {
            if self.segmented && self.should_leave() {
                return Err(Leave.into());
            }
            let idx = {
                let mut table = self.segs.lock_ok();
                if self.segmented {
                    // the last parts too are shared, in smaller pieces
                    segments::claim_work(&mut table, self.job.min_split)
                        .or_else(|| segments::split_largest(&mut table, self.job.min_split.min(TAIL_SPLIT)))
                } else {
                    let s = &mut table[0];
                    if s.active || s.is_finished() {
                        None
                    } else {
                        s.active = true;
                        Some(0)
                    }
                }
            };
            let Some(idx) = idx else { return Ok(()) };
            let result = self.download_segment(idx).await;
            {
                let mut table = self.segs.lock_ok();
                if let Some(s) = table.get_mut(idx) {
                    s.active = false;
                }
            }
            result?;
        }
    }
}
