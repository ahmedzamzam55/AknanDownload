use super::site::Site;
use super::worker::Worker;
use crate::http::{Progress, Shared};
use crate::segments::{self, Segment};
use crate::sync::LockOk;
use anyhow::{anyhow, bail};
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct FtpJob {
    pub url: String,
    pub auth: Option<(String, String)>,
    /// the path `look` found
    pub path: String,
    pub total_size: Option<u64>,
    pub rest: bool,
    pub part_path: PathBuf,
    pub connections: u32,
    pub min_split: u64,
    pub retries: u32,
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
}

/// The whole file into `job.part_path`, in parts on several connections when
/// the server allows. `segs` is the parts table (kept for resuming).
pub async fn run(
    shared: Arc<Shared>,
    job: FtpJob,
    segs: Arc<Mutex<Vec<Segment>>>,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let site = Arc::new(Site::new(&job.url, job.auth.as_ref())?);
    if let Some(dir) = job.part_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let segmented = job.rest && job.total_size.is_some();
    let file = OpenOptions::new().create(true).read(true).write(true).truncate(false).open(&job.part_path)?;
    {
        let mut table = segs.lock_ok();
        if segmented {
            let total = job.total_size.unwrap_or(0);
            if table.is_empty() {
                *table = segments::plan(total, job.connections.max(1), job.min_split);
                file.set_len(0)?;
            }
            if file.metadata()?.len() != total {
                crate::fileio::make_sparse(&file);
                file.set_len(total)?;
            }
        } else {
            file.set_len(0)?;
            *table = vec![Segment::new(0, job.total_size.unwrap_or(u64::MAX))];
        }
        for s in table.iter_mut() {
            s.active = false;
            s.on_disk = Some(s.pos);
        }
        progress.bytes.store(segments::downloaded(&table), Ordering::Relaxed);
    }
    let file = Arc::new(file);
    let child = cancel.child_token();
    let wanted = if segmented { job.connections.clamp(1, 16) } else { 1 };
    let mut set = tokio::task::JoinSet::new();
    for n in 0..wanted {
        let w = Worker {
            shared: shared.clone(),
            job: job.clone(),
            site: site.clone(),
            segs: segs.clone(),
            progress: progress.clone(),
            file: file.clone(),
            cancel: child.clone(),
            segmented,
            first: n == 0,
        };
        set.spawn(w.work());
    }
    let mut first_error = None;
    let mut to_disk = tokio::time::interval(Duration::from_secs(4));
    to_disk.tick().await;
    loop {
        tokio::select! {
            r = set.join_next() => {
                let Some(r) = r else { break };
                match r.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r) {
                    Ok(()) => {}
                    Err(e) if e.is::<TooMany>() => {}
                    Err(e) => {
                        if first_error.is_none() && !cancel.is_cancelled() {
                            first_error = Some(e);
                        }
                        child.cancel();
                    }
                }
            }
            _ = to_disk.tick() => {
                let seen: Vec<u64> = segs.lock_ok().iter().map(|s| s.pos).collect();
                let f = file.clone();
                if tokio::task::spawn_blocking(move || f.sync_data().is_ok()).await.unwrap_or(false) {
                    for (s, pos) in segs.lock_ok().iter_mut().zip(seen) {
                        s.on_disk = Some(pos.min(s.pos));
                    }
                }
            }
        }
    }
    if file.sync_all().is_ok() {
        for s in segs.lock_ok().iter_mut() {
            s.on_disk = None;
        }
    }
    if cancel.is_cancelled() {
        return Err(crate::http_download::Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    let table = segs.lock_ok();
    if segmented && !segments::all_finished(&table) {
        bail!("download ended before all parts were received");
    }
    Ok(())
}

/// The server allows fewer connections: this one stops, the others go on.
#[derive(Debug)]
pub(super) struct TooMany;
impl std::fmt::Display for TooMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the FTP server allows fewer connections")
    }
}
impl std::error::Error for TooMany {}
