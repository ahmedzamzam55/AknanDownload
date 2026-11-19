//! HLS (.m3u8) video streams: download every part with up to 32 adaptive
//! connections, decrypt AES-128 parts/maps when needed, strip spoofed headers,
//! and join parts without discarding `.sgparts` before post-processing finishes.

use crate::http::{Progress, Shared};
use crate::http_download::{learned_limit, Cancelled};
use crate::model::RequestHeaders;
use anyhow::{anyhow, bail};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct HlsJob {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
    /// final output file (written as .sgpart first by the caller)
    pub output: PathBuf,
    pub temp_dir: PathBuf,
    pub connections: u32,
    pub retries: u32,
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
    /// only the parts that play between these seconds ("Download a part")
    pub clip: Option<(f64, f64)>,
    /// optional pre-fetched playlist text (e.g. disguised #EXTM3U from probe)
    pub initial_playlist: Option<String>,
}

mod fetch;
mod prep;
pub mod sanitize;
mod worker;

use fetch::{join_parts, part_file};
pub use sanitize::{HlsContainer, HlsOutcome};
use worker::{ramp_task, run_worker, HlsPool};

async fn run_single(
    shared: Arc<Shared>,
    job: Arc<HlsJob>,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<(f64, HlsContainer, Option<String>)> {
    let (playlist, url, audio_url) = prep::load_playlist(&shared, &job).await?;
    let (chosen, first_at) = match job.clip {
        Some((a, b)) => {
            let (c, at) = playlist.parts_between(a, b);
            if c.is_empty() {
                bail!("the part asked for is after the end of the video");
            }
            (c, at)
        }
        None => ((0..playlist.parts.len()).collect(), 0.0),
    };
    prep::prepare_resume_dir(&job.temp_dir, &url, playlist.parts.len())?;

    let mut keys: HashMap<String, Vec<u8>> = HashMap::new();
    for p in chosen.iter().map(|&i| &playlist.parts[i]) {
        for k in p.key.iter().chain(p.init_key.iter()) {
            if !keys.contains_key(&k.uri) {
                let bytes = prep::fetch_key_bytes(&shared, &job, &k.uri, &cancel).await?;
                keys.insert(k.uri.clone(), bytes);
            }
        }
    }
    let keys = Arc::new(keys);
    let parts = Arc::new(playlist.parts);
    let chosen = Arc::new(chosen);

    let total = chosen.len();
    progress.reset_hls_workers();
    progress.parts_total.store(total as u32, Ordering::Relaxed);
    let (mut done_bytes, mut done_dur_ms, mut done) = (0u64, 0u64, 0u32);
    let mut todo = VecDeque::new();
    for &i in chosen.iter() {
        if let Ok(m) = std::fs::metadata(part_file(&job.temp_dir, i)) {
            if m.len() > 0 {
                done += 1;
                done_bytes += m.len();
                done_dur_ms += (parts[i].duration.max(0.0) * 1000.0) as u64;
                continue;
            }
        }
        todo.push_back(i);
    }
    progress.parts_done.store(done, Ordering::Relaxed);
    progress.bytes.store(done_bytes, Ordering::Relaxed);
    progress.hls_workers[0].0.store(done_bytes, Ordering::Relaxed);

    let total_dur_ms: u64 = chosen.iter().map(|&i| (parts[i].duration.max(0.0) * 1000.0) as u64).sum();
    let sample_url = chosen.first().map(|&i| parts[i].url.clone()).unwrap_or(url);
    let max_workers = job.connections.clamp(1, 32).min(todo.len().max(1) as u32) as usize;
    let learned = learned_limit(&sample_url).map(|n| (n as usize).clamp(1, max_workers));
    let initial_want = learned.unwrap_or_else(|| max_workers.min(2));
    let ceiling = learned.unwrap_or(max_workers);
    let pool = Arc::new(HlsPool {
        shared: shared.clone(),
        job: job.clone(),
        progress: progress.clone(),
        parts: parts.clone(),
        keys: keys.clone(),
        sample_url,
        queue: Mutex::new(todo),
        busy: AtomicUsize::new(0),
        want: AtomicUsize::new(initial_want),
        ceiling: AtomicUsize::new(ceiling),
        max_workers,
        ok_active: AtomicUsize::new(0),
        peak_ok: AtomicUsize::new(0),
        consec_403: AtomicU32::new(0),
        last_throttled: AtomicU64::new(0),
        finished: AtomicBool::new(false),
        wake: Notify::new(),
        done_bytes: AtomicU64::new(done_bytes),
        done_dur_ms: AtomicU64::new(done_dur_ms),
        done_parts: AtomicU32::new(done),
        total_dur_ms,
        total_parts: total as u64,
    });
    pool.update_estimate();

    if !pool.queue.lock().map(|q| q.is_empty()).unwrap_or(true) {
        let child = cancel.child_token();
        let ramp = tokio::spawn(ramp_task(pool.clone(), child.clone()));
        let mut set = JoinSet::new();
        for slot in 0..max_workers {
            set.spawn(run_worker(pool.clone(), slot, child.clone()));
        }
        let mut first_error = None;
        while let Some(r) = set.join_next().await {
            let r = r.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r);
            if let Err(e) = r {
                if first_error.is_none() && !cancel.is_cancelled() {
                    first_error = Some(e);
                }
                child.cancel();
            }
        }
        ramp.abort();
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        if let Some(e) = first_error {
            return Err(e);
        }
    }
    progress.est_total.store(pool.done_bytes.load(Ordering::Relaxed), Ordering::Relaxed);
    let container = join_parts(&shared, &job, &progress, parts, keys, chosen).await?;
    progress.assembling.store(false, Ordering::Relaxed);
    Ok((first_at, container, audio_url))
}

/// Download the HLS stream (and separate `#EXT-X-MEDIA:TYPE=AUDIO` if present)
/// into `job.output`. Caller deletes `outcome.temp_dir` after finalization.
pub async fn run(
    shared: Arc<Shared>,
    job: HlsJob,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<HlsOutcome> {
    let temp_dir = job.temp_dir.clone();
    let job_arc = Arc::new(job);
    let (first_at, container, audio_url) = run_single(shared.clone(), job_arc.clone(), progress.clone(), cancel.clone()).await?;
    let mut audio_output = None;
    if let Some(au) = audio_url {
        let a_out = temp_dir.join("audio.joined");
        let a_job = Arc::new(HlsJob {
            url: au,
            headers: job_arc.headers.clone(),
            user_agent: job_arc.user_agent.clone(),
            output: a_out.clone(),
            temp_dir: temp_dir.join("audio_parts"),
            connections: job_arc.connections,
            retries: job_arc.retries,
            limiter: job_arc.limiter.clone(),
            clip: job_arc.clip,
            initial_playlist: None,
        });
        let _ = run_single(shared, a_job, Arc::new(Progress::default()), cancel).await?;
        if std::fs::metadata(&a_out).map(|m| m.len() > 0).unwrap_or(false) {
            audio_output = Some(a_out);
        }
    }
    Ok(HlsOutcome { first_at, container, audio_output, temp_dir })
}
