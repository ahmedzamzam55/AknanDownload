//! HLS (.m3u8) video streams: download every part with several connections,
//! decrypt AES-128 parts when needed, then join them into one video file.
//!
//! DRM protected streams (SAMPLE-AES, Widevine, FairPlay...) are refused on
//! purpose. Live streams (no #EXT-X-ENDLIST) are refused too.

use crate::http::{ConnGuard, Progress, Shared};
use crate::http_download::Cancelled;
use crate::model::RequestHeaders;
use anyhow::{anyhow, bail, Context};
use std::collections::HashMap;
use crate::m3u8::{best_variant, parse_media};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
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
use fetch::{decrypt, fetch_bytes, fetch_full, fetch_text, join_parts, part_file};

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
    progress.parts_total.store(total as u32, Ordering::Relaxed);
    let mut done_bytes = 0u64;
    let mut done = 0u32;
    for &i in chosen.iter() {
        if let Ok(m) = std::fs::metadata(part_file(&job.temp_dir, i)) {
            done += 1;
            done_bytes += m.len();
        }
    }
    progress.parts_done.store(done, Ordering::Relaxed);
    progress.bytes.store(done_bytes, Ordering::Relaxed);

    let next = Arc::new(AtomicUsize::new(0));
    let child = cancel.child_token();
    let mut set = JoinSet::new();
    for _ in 0..job.connections.clamp(1, 16) {
        let (shared, job, progress, child, next, parts, keys, chosen) = (
            shared.clone(),
            job.clone(),
            progress.clone(),
            child.clone(),
            next.clone(),
            parts.clone(),
            keys.clone(),
            chosen.clone(),
        );
        set.spawn(async move {
            loop {
                let n = next.fetch_add(1, Ordering::Relaxed);
                if n >= chosen.len() {
                    return Ok::<(), anyhow::Error>(());
                }
                let i = chosen[n];
                let path = part_file(&job.temp_dir, i);
                if path.exists() {
                    continue;
                }
                let mut attempt = 0;
                loop {
                    let fut = async {
                        let _g = ConnGuard::new(&progress);
                        let mut data = fetch_full(&shared, &job, &parts[i].url, parts[i].range).await?.0;
                        shared.limiter.acquire(data.len()).await;
                        job.limiter.acquire(data.len()).await;
                        if let Some(k) = &parts[i].key {
                            let key = keys.get(&k.uri).ok_or_else(|| anyhow!("missing key"))?;
                            let iv = k
                                .iv
                                .unwrap_or_else(|| (parts[i].sequence as u128).to_be_bytes());
                            decrypt(key, &iv, &mut data)?;
                        }
                        // on the disk before it counts as there (a power
                        // cut left empty pieces that were taken as done)
                        let tmp = path.with_extension("tmp");
                        {
                            use std::io::Write;
                            let mut f = std::fs::File::create(&tmp)?;
                            f.write_all(&data)?;
                            f.sync_data()?;
                        }
                        std::fs::rename(&tmp, &path)?;
                        Ok::<u64, anyhow::Error>(data.len() as u64)
                    };
                    let r = tokio::select! {
                        r = fut => r,
                        _ = child.cancelled() => return Err(Cancelled.into()),
                    };
                    match r {
                        Ok(n) => {
                            progress.bytes.fetch_add(n, Ordering::Relaxed);
                            progress.parts_done.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                        Err(e) => {
                            attempt += 1;
                            if attempt > job.retries {
                                return Err(e);
                            }
                            tokio::time::sleep(Duration::from_secs((attempt as u64 * 2).min(20))).await;
                        }
                    }
                }
            }
        });
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
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    join_parts(&shared, &job, &progress, playlist.init_url.as_deref(), playlist.init_range, chosen).await?;
    progress.assembling.store(false, Ordering::Relaxed);
    Ok(first_at)
}
