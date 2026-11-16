use super::learned::{has_work, now_secs};
use super::prepare::open_part_file;
use super::types::{Cancelled, Crew, HttpJob, SegTable, CLIMB_AFTER};
use super::worker::Worker;
use crate::fileio::set_hidden;
use crate::http::{Progress, Shared};
use crate::segments;
use crate::sync::LockOk;
use anyhow::{anyhow, bail};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub async fn run(
    shared: Arc<Shared>,
    job: HttpJob,
    segs: SegTable,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let part = job.part_path.clone();
    let r = run_hidden(shared, job, segs, progress, cancel).await;
    // however it ended: the temporary file is shown again
    set_hidden(&part, false);
    r
}

async fn run_hidden(
    shared: Arc<Shared>,
    job: HttpJob,
    segs: SegTable,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    if let Some(dir) = job.part_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let segmented = job.resumable && job.total_size.is_some();
    let file = open_part_file(&job, &segs, &progress, segmented)?;
    let file = Arc::new(file);
    let child = cancel.child_token();
    let crew = Arc::new(Mutex::new(Crew::default()));
    let wanted = {
        let (progress, job_conns) = (progress.clone(), job.connections.max(1));
        move || {
            if !segmented {
                return 1;
            }
            match progress.want_connections.load(Ordering::Relaxed) {
                0 => job_conns,
                n => n.clamp(1, 64),
            }
        }
    };
    let mut set = JoinSet::new();
    let spawn = |set: &mut JoinSet<anyhow::Result<()>>| {
        crew.lock_ok().running += 1;
        let ctx = Worker {
            shared: shared.clone(),
            job: job.clone(),
            segs: segs.clone(),
            progress: progress.clone(),
            file: file.clone(),
            cancel: child.clone(),
            segmented,
            crew: crew.clone(),
            wanted: Arc::new(wanted.clone()),
            retry_after: std::sync::atomic::AtomicU64::new(0),
        };
        set.spawn(ctx.run());
    };
    for _ in 0..wanted() {
        spawn(&mut set);
    }

    let mut first_error: Option<anyhow::Error> = None;
    // every few seconds what was written is made sure to be on the disk
    // (and the list may save that far)
    let mut to_disk = tokio::time::interval(Duration::from_secs(4));
    to_disk.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    to_disk.tick().await;
    let mut look = tokio::time::interval(Duration::from_millis(500));
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // what one connection brings at best (measured every 2 s over the whole
    // download)
    let mut sample = (std::time::Instant::now(), progress.bytes.load(Ordering::Relaxed));
    loop {
        tokio::select! {
            res = set.join_next() => {
                let Some(res) = res else {
                    // every connection is done: more work left (fewer were
                    // wanted for a moment, then more again)? then on
                    if first_error.is_none() && !cancel.is_cancelled() && segmented && wanted() > 0 {
                        let more = !segments::all_finished(&segs.lock_ok());
                        if more {
                            spawn(&mut set);
                            continue;
                        }
                    }
                    break;
                };
                let res = res.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r);
                if let Err(e) = res {
                    if first_error.is_none() && !cancel.is_cancelled() {
                        first_error = Some(e);
                    }
                    child.cancel();
                }
            }
            _ = to_disk.tick() => {
                let seen: Vec<u64> = segs.lock_ok().iter().map(|s| s.pos).collect();
                let f = file.clone();
                let synced = tokio::task::spawn_blocking(move || f.sync_data().is_ok()).await.unwrap_or(false);
                if synced {
                    let mut table = segs.lock_ok();
                    for (s, pos) in table.iter_mut().zip(seen) {
                        s.on_disk = Some(pos.min(s.pos));
                    }
                }
            }
            _ = look.tick() => {
                let spent = sample.0.elapsed();
                if spent >= Duration::from_secs(2) {
                    let now = progress.bytes.load(Ordering::Relaxed);
                    let conns = progress.connections.load(Ordering::Relaxed).max(1) as u64;
                    let per = now.saturating_sub(sample.1) * 1000 / (spent.as_millis().max(1) as u64) / conns;
                    let mut c = crew.lock_ok();
                    c.best = c.best.max(per);
                    drop(c);
                    sample = (std::time::Instant::now(), now);
                }
                // the server made it take fewer connections: a minute later
                // without that, one more is tried (it lowers again if not)
                if segmented {
                    let at = progress.lowered_at.load(Ordering::Relaxed);
                    let want = progress.want_connections.load(Ordering::Relaxed);
                    let began = job.connections.max(1);
                    if at > 0 && want > 0 && want < began && now_secs().saturating_sub(at) >= CLIMB_AFTER {
                        progress.want_connections.store(want + 1, Ordering::Relaxed);
                        progress.lowered_at.store(if want + 1 >= began { 0 } else { now_secs() }, Ordering::Relaxed);
                    }
                }
                // more connections wanted (Options): as many more as there are
                // parts worth sharing
                if first_error.is_none() && segmented && !child.is_cancelled() {
                    let (running, leaving) = {
                        let c = crew.lock_ok();
                        (c.running, c.leaving)
                    };
                    let missing = wanted().saturating_sub(running.saturating_sub(leaving));
                    for _ in 0..missing {
                        if !has_work(&segs, job.min_split) {
                            break;
                        }
                        spawn(&mut set);
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
        return Err(Cancelled.into());
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
