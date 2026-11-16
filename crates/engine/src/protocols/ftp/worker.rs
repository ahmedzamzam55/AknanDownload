use super::control::{Control, FtpReply};
use super::site::Site;
use super::transfer::{FtpJob, TooMany};
use super::ANSWER;
use crate::http::{reconnect_delay, ConnGuard, NetDrop, Progress, Shared};
use crate::segments::{self, Segment};
use crate::sync::LockOk;
use std::fs::File;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(super) struct Worker {
    pub(super) shared: Arc<Shared>,
    pub(super) job: FtpJob,
    pub(super) site: Arc<Site>,
    pub(super) segs: Arc<Mutex<Vec<Segment>>>,
    pub(super) progress: Arc<Progress>,
    pub(super) file: Arc<File>,
    pub(super) cancel: CancellationToken,
    pub(super) segmented: bool,
    /// the first connection: never the one that gives way
    pub(super) first: bool,
}

impl Worker {
    fn table(&self) -> std::sync::MutexGuard<'_, Vec<Segment>> {
        self.segs.lock_ok()
    }

    pub(super) async fn work(self) -> anyhow::Result<()> {
        loop {
            let idx = {
                let mut t = self.table();
                if self.segmented {
                    segments::claim_work(&mut t, self.job.min_split).or_else(|| segments::split_largest(&mut t, self.job.min_split.min(64 * 1024)))
                } else if t[0].active || t[0].is_finished() {
                    None
                } else {
                    t[0].active = true;
                    Some(0)
                }
            };
            let Some(idx) = idx else { return Ok(()) };
            let r = self.segment(idx).await;
            if let Some(s) = self.table().get_mut(idx) {
                s.active = false;
            }
            r?;
        }
    }

    async fn segment(&self, idx: usize) -> anyhow::Result<()> {
        let (mut fails, mut net) = (0u32, 0u32);
        loop {
            if self.cancel.is_cancelled() {
                return Err(crate::http_download::Cancelled.into());
            }
            let before = self.table()[idx].pos;
            let r = tokio::select! {
                r = self.fetch(idx) => r,
                _ = self.cancel.cancelled() => return Err(crate::http_download::Cancelled.into()),
            };
            let e = match r {
                Ok(()) if self.table()[idx].is_finished() => return Ok(()),
                Ok(()) => continue,
                Err(e) => e,
            };
            if self.table()[idx].pos > before {
                fails = 0;
                net = 0;
            }
            let code = e.chain().find_map(|c| c.downcast_ref::<FtpReply>().map(|r| r.code));
            // 421 / 530 "too many users": one connection less
            if !self.first && matches!(code, Some(421)) {
                return Err(TooMany.into());
            }
            let network = crate::http::is_network_error(&e) || matches!(code, Some(421 | 425 | 426 | 450));
            if network {
                net += 1;
                self.progress.waiting.store(true, Ordering::Relaxed);
            } else {
                fails += 1;
                if matches!(code, Some(550 | 553)) || fails > self.job.retries {
                    return Err(e);
                }
            }
            if !self.segmented {
                // no resume: from the start again
                let mut t = self.table();
                self.progress.bytes.fetch_sub(t[idx].done(), Ordering::Relaxed);
                t[idx].pos = t[idx].start;
                let _ = self.file.set_len(0);
            }
            let wait = if network { reconnect_delay(net) } else { Duration::from_secs((1u64 << fails.min(5)).min(30)) };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.progress.back_online.notified() => {}
                _ = self.cancel.cancelled() => return Err(crate::http_download::Cancelled.into()),
            }
        }
    }

    async fn fetch(&self, idx: usize) -> anyhow::Result<()> {
        let pos = self.table()[idx].pos;
        let mut c = Control::connect(&self.site).await?;
        let mut data = c.open(&self.job.path, pos).await?;
        let _guard = ConnGuard::new(&self.progress);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match tokio::time::timeout(ANSWER, data.read(&mut buf)).await {
                Ok(r) => r?,
                Err(_) => return Err(NetDrop("connection timed out").into()),
            };
            if n == 0 {
                break;
            }
            self.shared.limiter.acquire(n).await;
            self.job.limiter.acquire(n).await;
            let done = {
                let mut t = self.table();
                let s = &mut t[idx];
                let take = (n as u64).min(s.end.saturating_sub(s.pos)) as usize;
                if take > 0 {
                    crate::fileio::write_at(&self.file, &buf[..take], s.pos)?;
                    s.pos += take as u64;
                    self.progress.bytes.fetch_add(take as u64, Ordering::Relaxed);
                    self.progress.online();
                }
                s.pos >= s.end
            };
            if done {
                // the part is here (another connection has the rest): this
                // transfer is let go
                return Ok(());
            }
        }
        // the whole rest of the file came
        let finished = tokio::time::timeout(ANSWER, c.reply()).await;
        let mut t = self.table();
        let s = &mut t[idx];
        if !self.segmented && (self.job.total_size.is_none() || s.end == u64::MAX) {
            s.end = s.pos;
            return Ok(());
        }
        if s.pos >= s.end {
            return Ok(());
        }
        match finished {
            Ok(Ok(r)) if r.code >= 400 => Err(FtpReply { code: r.code, text: r.text }.into()),
            _ => Err(NetDrop("connection closed early").into()),
        }
    }
}
