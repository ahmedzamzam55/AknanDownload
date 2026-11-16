use crate::{clip, clipserve};
use crate::hls::{self, HlsJob};
use crate::http::{self, Progress, Shared};
use crate::http_download::Cancelled;
use crate::limiter::SpeedLimiter;
use crate::model::{Clip, DownloadKind, RequestHeaders, Step};
use crate::mux;
use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Where a part of a video comes from.
#[derive(Debug, Clone)]
pub struct ClipSource {
    pub url: String,
    /// the sound apart (video pages)
    pub audio_url: Option<String>,
    pub headers: RequestHeaders,
    /// a stream (.m3u8): its parts are fetched, not read by ffmpeg
    pub hls: bool,
}

/// Where the work of a cut goes (streams).
pub(super) struct ClipWork {
    pub(super) joined: PathBuf,
    pub(super) parts: PathBuf,
    pub(super) connections: u32,
    pub(super) retries: u32,
    pub(super) user_agent: String,
}

/// Only `c` of `src` into `output` (see `run_clip`).
#[allow(clippy::too_many_arguments)]
pub(super) async fn cut_source(
    shared: Arc<Shared>,
    src: &ClipSource,
    c: Clip,
    out: &clip::Output,
    output: &Path,
    work: &ClipWork,
    limiter: Arc<SpeedLimiter>,
    progress: Arc<Progress>,
    cancel: &CancellationToken,
) -> anyhow::Result<clip::Done> {
    // ffmpeg missing: fetched again first
    crate::tools::restore_ffmpeg(&shared.client()).await;
    let ffmpeg = mux::find_ffmpeg().ok_or_else(|| anyhow!("{}", mux::MISSING))?;
    let mut tickets = Vec::new();
    // a stream behind an address that does not say so (".m3u8")
    let mut hls = src.hls;
    if !hls && src.audio_url.is_none() {
        let client = shared.client();
        let p = tokio::select! {
            r = http::probe(&client, &src.url, &src.headers, &work.user_agent) => r.ok(),
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        hls = p.map(|p| p.kind == DownloadKind::Hls).unwrap_or(false);
    }
    let (inputs, from) = if hls {
        let job = HlsJob {
            url: src.url.clone(),
            headers: src.headers.clone(),
            user_agent: work.user_agent.clone(),
            output: work.joined.clone(),
            temp_dir: work.parts.clone(),
            connections: work.connections,
            retries: work.retries,
            limiter,
            clip: Some((c.start, c.end)),
        };
        let first_at = hls::run(shared.clone(), job, progress.clone(), cancel.clone()).await?;
        progress.assembling.store(true, Ordering::Relaxed);
        progress.set_step(Some(Step::Cut));
        (vec![work.joined.to_string_lossy().into_owned()], (c.start - first_at).max(0.0))
    } else {
        let mut inputs = Vec::new();
        for url in std::iter::once(src.url.clone()).chain(src.audio_url.clone()) {
            let s = clipserve::Source { url, headers: src.headers.clone(), user_agent: work.user_agent.clone() };
            let t = clipserve::open(shared.clone(), s, progress.clone()).await?;
            inputs.push(t.url.clone());
            tickets.push(t);
        }
        progress.parts_total.store(1000, Ordering::Relaxed);
        progress.parts_done.store(0, Ordering::Relaxed);
        progress.connections.store(inputs.len() as u32, Ordering::Relaxed);
        (inputs, c.start)
    };
    let remote = !hls;
    let p2 = progress.clone();
    let result = clip::cut(
        &ffmpeg,
        &inputs,
        from,
        c.len(),
        c.exact,
        out,
        output,
        move |f| {
            if remote {
                p2.parts_done.store((f * 1000.0) as u32, Ordering::Relaxed);
            } else {
                p2.step_at(f);
            }
        },
        cancel,
    )
    .await;
    drop(tickets);
    progress.connections.store(0, Ordering::Relaxed);
    progress.set_step(None);
    if hls && result.is_ok() {
        let _ = std::fs::remove_file(&work.joined);
    }
    result
}
