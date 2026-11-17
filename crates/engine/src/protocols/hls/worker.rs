use super::fetch::{fetch_part_stream, part_file};
use super::HlsJob;
use crate::http::{http_status_of, is_network_error, ConnGuard, Progress, Shared};
use crate::http_download::{learn, raise_learned, Cancelled};
use crate::m3u8::Part;
use crate::sync::LockOk;
use anyhow::anyhow;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct ActiveGuard<'a>(&'a AtomicBool);
impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

pub(super) struct HlsPool {
    pub shared: Arc<Shared>,
    pub job: Arc<HlsJob>,
    pub progress: Arc<Progress>,
    pub parts: Arc<Vec<Part>>,
    pub keys: Arc<HashMap<String, Vec<u8>>>,
    pub sample_url: String,
    pub queue: Mutex<VecDeque<usize>>,
    pub busy: AtomicUsize,
    pub want: AtomicUsize,
    pub ceiling: AtomicUsize,
    pub max_workers: usize,
    pub ok_active: AtomicUsize,
    pub peak_ok: AtomicUsize,
    pub last_throttled: AtomicU64,
    pub finished: AtomicBool,
    pub wake: Notify,
    pub done_bytes: AtomicU64,
    pub done_dur_ms: AtomicU64,
    pub done_parts: AtomicU32,
    pub total_dur_ms: u64,
    pub total_parts: u64,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl HlsPool {
    pub(super) fn update_estimate(&self) {
        let db = self.done_bytes.load(Ordering::Relaxed);
        let dd = self.done_dur_ms.load(Ordering::Relaxed);
        let dp = self.done_parts.load(Ordering::Relaxed) as u64;
        if dp == 0 || db == 0 {
            return;
        }
        let est = if self.total_dur_ms > 0 && dd > 0 {
            ((db as u128 * self.total_dur_ms as u128) / dd as u128) as u64
        } else {
            ((db as u128 * self.total_parts.max(1) as u128) / dp as u128) as u64
        };
        let now = self.progress.bytes.load(Ordering::Relaxed);
        self.progress.est_total.store(est.max(now), Ordering::Relaxed);
    }

    fn take_work(&self) -> Option<usize> {
        let mut q = self.queue.lock_ok();
        let next = q.pop_front()?;
        self.busy.fetch_add(1, Ordering::SeqCst);
        Some(next)
    }

    fn push_back(&self, i: usize, done: bool) {
        if !done {
            self.queue.lock_ok().push_front(i);
        }
        let rem_busy = self.busy.fetch_sub(1, Ordering::SeqCst) - 1;
        if rem_busy == 0 && self.queue.lock_ok().is_empty() {
            self.finished.store(true, Ordering::Release);
            self.wake.notify_waiters();
        }
    }
}

/// Gradually scales HLS concurrency up when open connections are accepted cleanly,
/// so burst-limited CDNs discover their true ceiling without tripping `503`.
pub(super) async fn ramp_task(pool: Arc<HlsPool>, cancel: CancellationToken) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            _ = cancel.cancelled() => return,
        }
        if pool.finished.load(Ordering::Acquire) || pool.queue.lock_ok().is_empty() {
            return;
        }
        let cur_want = pool.want.load(Ordering::Relaxed);
        let ok = pool.ok_active.load(Ordering::Relaxed);
        if ok >= 2 {
            raise_learned(&pool.sample_url, ok as u32);
        }
        if cur_want >= pool.max_workers || ok < cur_want {
            continue;
        }
        let ceil = pool.ceiling.load(Ordering::Relaxed);
        if cur_want < ceil {
            let next = if cur_want < 4 {
                cur_want + 1
            } else if cur_want < 8 {
                (cur_want + 2).min(ceil)
            } else {
                (cur_want + 4).min(ceil)
            };
            pool.want.store(next, Ordering::Relaxed);
            pool.wake.notify_waiters();
        } else if now_secs().saturating_sub(pool.last_throttled.load(Ordering::Relaxed)) >= 20 {
            let next = (cur_want + 1).min(pool.max_workers);
            pool.ceiling.store(next, Ordering::Relaxed);
            pool.want.store(next, Ordering::Relaxed);
            pool.wake.notify_waiters();
        }
    }
}

pub(super) async fn run_worker(pool: Arc<HlsPool>, slot: usize, cancel: CancellationToken) -> anyhow::Result<()> {
    if slot > 0 && slot < pool.want.load(Ordering::Relaxed) {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(120 * slot.min(4) as u64)) => {}
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        }
    }
    loop {
        while slot >= pool.want.load(Ordering::Relaxed) {
            if pool.finished.load(Ordering::Acquire) {
                return Ok(());
            }
            tokio::select! {
                _ = pool.wake.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                _ = cancel.cancelled() => return Err(Cancelled.into()),
            }
        }
        let Some(i) = pool.take_work() else {
            if pool.busy.load(Ordering::SeqCst) == 0 {
                pool.finished.store(true, Ordering::Release);
                pool.wake.notify_waiters();
            }
            return Ok(());
        };
        let path = part_file(&pool.job.temp_dir, i);
        if path.exists() {
            pool.push_back(i, true);
            continue;
        }
        let mut attempt = 0u32;
        loop {
            let r = {
                pool.progress.hls_workers[slot].1.store(true, Ordering::Relaxed);
                let _ag = ActiveGuard(&pool.progress.hls_workers[slot].1);
                let _cg = ConnGuard::new(&pool.progress);
                let key = match &pool.parts[i].key {
                    Some(k) => Some(pool.keys.get(&k.uri).ok_or_else(|| anyhow!("missing key"))?.as_slice()),
                    None => None,
                };
                let fut = fetch_part_stream(
                    &pool.shared,
                    &pool.job,
                    &pool.progress,
                    &pool.ok_active,
                    &pool.peak_ok,
                    slot,
                    &pool.parts[i],
                    key,
                    &path,
                );
                tokio::select! {
                    r = fut => r,
                    _ = cancel.cancelled() => Err(Cancelled.into()),
                }
            };
            match r {
                Ok(n) => {
                    pool.done_bytes.fetch_add(n, Ordering::Relaxed);
                    let ms = (pool.parts[i].duration.max(0.0) * 1000.0) as u64;
                    pool.done_dur_ms.fetch_add(ms, Ordering::Relaxed);
                    pool.done_parts.fetch_add(1, Ordering::Relaxed);
                    pool.progress.parts_done.fetch_add(1, Ordering::Relaxed);
                    pool.update_estimate();
                    pool.push_back(i, true);
                    break;
                }
                Err(e) if cancel.is_cancelled() || e.is::<Cancelled>() => {
                    pool.push_back(i, false);
                    return Err(Cancelled.into());
                }
                Err(e) => {
                    let overloaded = matches!(http_status_of(&e), Some(429 | 503))
                        || (is_network_error(&e) && pool.ok_active.load(Ordering::Relaxed) >= 1);
                    let cur_want = pool.want.load(Ordering::Relaxed);
                    if overloaded && cur_want > 1 {
                        pool.last_throttled.store(now_secs(), Ordering::Relaxed);
                        let ok_now = pool.ok_active.load(Ordering::Relaxed);
                        let live = ok_now.max(pool.peak_ok.load(Ordering::Relaxed));
                        let cap = if live >= 1 {
                            live.min(cur_want - 1)
                        } else {
                            (cur_want / 2).max(1)
                        };
                        pool.ceiling.fetch_min(cap, Ordering::Relaxed);
                        pool.want.fetch_min(cap, Ordering::Relaxed);
                        if cap >= 2 || ok_now >= 1 {
                            learn(&pool.parts[i].url, cap as u32);
                        }
                        if slot >= pool.want.load(Ordering::Relaxed) {
                            pool.push_back(i, false);
                            break;
                        }
                    }
                    attempt += 1;
                    if attempt > pool.job.retries {
                        pool.push_back(i, false);
                        return Err(e);
                    }
                    let wait = Duration::from_secs((attempt as u64 * 2).min(12));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => {
                            pool.push_back(i, false);
                            return Err(Cancelled.into());
                        }
                    }
                }
            }
        }
    }
}
