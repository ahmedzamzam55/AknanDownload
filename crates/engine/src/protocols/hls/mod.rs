//! HLS (.m3u8) video streams: download every part with several connections,
//! decrypt AES-128 parts when needed, then join them into one video file.
//!
//! DRM protected streams (SAMPLE-AES, Widevine, FairPlay...) are refused on
//! purpose. Live streams (no #EXT-X-ENDLIST) are refused too.

use crate::http::{Progress, Shared};
use crate::http_download::{learned_limit, Cancelled};
use crate::m3u8::{best_variant, parse_media};
use crate::model::RequestHeaders;
use anyhow::{anyhow, bail, Context};
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
}

mod fetch;
mod worker;
use fetch::{fetch_bytes, fetch_text, join_parts, part_file};
use worker::{ramp_task, run_worker, HlsPool};

/// Gets the stream (or only the parts of `job.clip`) into `job.output`.
/// Gives when the first part it took starts in the stream (seconds; 0
/// without a clip): the clip starts that much later in the file.
pub async fn run(
    shared: Arc<Shared>,
    job: HlsJob,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<f64> {
    let job = Arc::new(job);
    // 1. playlist (follow a master playlist to its best quality)
    // the addresses in a list are relative to where it came from in the end
    let (mut text, mut url) = fetch_text(&shared, &job, &job.url).await.context("could not load playlist")?;
    if !text.trim_start().starts_with("#EXTM3U") {
        bail!("not a valid HLS playlist");
    }
    if text.contains("#EXT-X-STREAM-INF") {
        let variant = best_variant(&url, &text)?.ok_or_else(|| anyhow!("no video variant found"))?;
        (text, url) = fetch_text(&shared, &job, &variant).await?;
    }
    let playlist = parse_media(&url, &text)?;
    if !playlist.ended {
        bail!("live streams are not supported (the playlist has no end)");
    }
    if playlist.parts.is_empty() {
        bail!("the playlist has no parts");
    }
    // a part of the video: only the parts that play then
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
    std::fs::create_dir_all(&job.temp_dir)?;

    // 2. keys (usually one for the whole stream)
    let mut keys: HashMap<String, Vec<u8>> = HashMap::new();
    for p in chosen.iter().map(|&i| &playlist.parts[i]) {
        if let Some(k) = &p.key {
            if !keys.contains_key(&k.uri) {
                let bytes = fetch_bytes(&shared, &job, &k.uri).await.context("could not load key")?;
                keys.insert(k.uri.clone(), bytes);
            }
        }
    }
    let keys = Arc::new(keys);
    let parts = Arc::new(playlist.parts);
    let chosen = Arc::new(chosen);

    // 3. parts, several at a time; finished parts survive pause/resume
    let total = chosen.len();
    progress.reset_hls_workers();
    progress.parts_total.store(total as u32, Ordering::Relaxed);
    let mut done_bytes = 0u64;
    let mut done_dur_ms = 0u64;
    let mut done = 0u32;
    let mut todo = VecDeque::new();
    for &i in chosen.iter() {
        if let Ok(m) = std::fs::metadata(part_file(&job.temp_dir, i)) {
            done += 1;
            done_bytes += m.len();
            done_dur_ms += (parts[i].duration.max(0.0) * 1000.0) as u64;
        } else {
            todo.push_back(i);
        }
    }
    progress.parts_done.store(done, Ordering::Relaxed);
    progress.bytes.store(done_bytes, Ordering::Relaxed);
    progress.hls_workers[0].0.store(done_bytes, Ordering::Relaxed);

    let total_dur_ms: u64 = chosen.iter().map(|&i| (parts[i].duration.max(0.0) * 1000.0) as u64).sum();
    let sample_url = chosen.first().map(|&i| parts[i].url.as_str()).unwrap_or(&url).to_string();
    let max_workers = job.connections.clamp(1, 16).min(todo.len().max(1) as u32) as usize;
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
    join_parts(&shared, &job, &progress, playlist.init_url.as_deref(), playlist.init_range, chosen).await?;
    progress.assembling.store(false, Ordering::Relaxed);
    Ok(first_at)
}
