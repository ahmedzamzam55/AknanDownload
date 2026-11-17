use super::fetch::{fetch_part_stream, part_file};
use super::HlsJob;
use crate::http::{http_status_of, is_network_error, ConnGuard, Progress, Shared};
use crate::http_download::{learn, Cancelled};
use crate::m3u8::Part;
use crate::sync::LockOk;
use anyhow::anyhow;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
    pub queue: Mutex<VecDeque<usize>>,
    pub want: AtomicUsize,
    pub ok_active: AtomicUsize,
    pub done_bytes: AtomicU64,
    pub done_dur_ms: AtomicU64,
    pub done_parts: AtomicU32,
    pub total_dur_ms: u64,
    pub total_parts: u64,
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
}

pub(super) async fn run_worker(pool: Arc<HlsPool>, slot: usize, cancel: CancellationToken) -> anyhow::Result<()> {
    if slot > 0 {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(40 * slot as u64)) => {}
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        }
    }
    loop {
        if slot >= pool.want.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(i) = pool.queue.lock_ok().pop_front() else {
            return Ok(());
        };
        let path = part_file(&pool.job.temp_dir, i);
        if path.exists() {
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
                    break;
                }
                Err(e) if cancel.is_cancelled() || e.is::<Cancelled>() => {
                    pool.queue.lock_ok().push_front(i);
                    return Err(Cancelled.into());
                }
                Err(e) => {
                    let overloaded = matches!(http_status_of(&e), Some(429 | 503))
                        || (is_network_error(&e) && pool.ok_active.load(Ordering::Relaxed) >= 1);
                    let cur_want = pool.want.load(Ordering::Relaxed);
                    if overloaded && cur_want > 1 {
                        let ok_now = pool.ok_active.load(Ordering::Relaxed);
                        let cap = if ok_now >= 1 { ok_now.min(cur_want - 1) } else { (cur_want / 2).max(1) };
                        pool.want.fetch_min(cap, Ordering::Relaxed);
                        learn(&pool.parts[i].url, cap as u32);
                        if slot >= pool.want.load(Ordering::Relaxed) {
                            pool.queue.lock_ok().push_front(i);
                            return Ok(());
                        }
                    }
                    attempt += 1;
                    if attempt > pool.job.retries {
                        pool.queue.lock_ok().push_front(i);
                        return Err(e);
                    }
                    let wait = Duration::from_secs((attempt as u64 * 2).min(12));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => {
                            pool.queue.lock_ok().push_front(i);
                            return Err(Cancelled.into());
                        }
                    }
                }
            }
        }
    }
}
