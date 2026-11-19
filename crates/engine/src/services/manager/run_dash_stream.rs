use crate::config::Settings;
use crate::hls::{self, HlsJob};
use crate::http::{Progress, Shared};
use crate::http_download::{self, HttpJob, SegTable};
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadKind, ProbeResult, RequestHeaders};
use crate::segments::Segment;
use crate::sync::LockOk;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) struct DashLeg<'a> {
    pub shared: &'a Arc<Shared>,
    pub probe: &'a ProbeResult,
    pub fallback_url: &'a str,
    pub headers: &'a RequestHeaders,
    pub part_path: PathBuf,
    pub hls_dir: PathBuf,
    pub segs: &'a SegTable,
    pub connections: u32,
    pub progress_base: u64,
    pub limiter: &'a Arc<SpeedLimiter>,
    pub progress: &'a Arc<Progress>,
    pub cancel: &'a CancellationToken,
    pub settings: &'a Settings,
}

/// Download either the video or audio leg of a DASH item, seamlessly
/// dispatching `.m3u8` HLS streams vs segmented HTTP byte-range downloads.
pub(super) async fn download_leg(leg: DashLeg<'_>) -> anyhow::Result<u64> {
    let url = if leg.probe.final_url.is_empty() {
        leg.fallback_url.to_string()
    } else {
        leg.probe.final_url.clone()
    };
    if leg.probe.kind == DownloadKind::Hls || crate::http::looks_like_hls(&url, None) {
        let base_bytes = leg.progress.bytes.load(Ordering::Relaxed);
        let hls_prog = Arc::new(Progress::default());
        let done_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fwd_done = done_flag.clone();
        let (hp, dp, base) = (hls_prog.clone(), leg.progress.clone(), leg.progress_base);
        let fwd = tokio::spawn(async move {
            while !fwd_done.load(Ordering::Relaxed) {
                dp.bytes.store(base.saturating_add(hp.bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
        let job = HlsJob {
            url,
            headers: leg.headers.clone(),
            user_agent: leg.settings.user_agent.clone(),
            output: leg.part_path.clone(),
            temp_dir: leg.hls_dir,
            connections: leg.connections,
            retries: leg.settings.retries,
            limiter: leg.limiter.clone(),
            clip: None,
            initial_playlist: leg.probe.hls_body.clone(),
        };
        let res = hls::run(leg.shared.clone(), job, hls_prog.clone(), leg.cancel.clone()).await;
        done_flag.store(true, Ordering::Relaxed);
        let _ = fwd.await;
        let out = res?;
        let _ = std::fs::remove_dir_all(&out.temp_dir);
        let final_len = std::fs::metadata(&leg.part_path).map(|m| m.len()).unwrap_or_else(|_| hls_prog.bytes.load(Ordering::Relaxed));
        leg.progress.bytes.store(leg.progress_base.saturating_add(final_len).max(base_bytes), Ordering::Relaxed);
        if final_len > 0 {
            let mut s = Segment::new(0, final_len);
            s.pos = final_len;
            *leg.segs.lock_ok() = vec![s];
        }
        return Ok(final_len);
    }

    let job = HttpJob {
        url: url.clone(),
        headers: leg.headers.clone(),
        user_agent: leg.settings.user_agent.clone(),
        total_size: leg.probe.total_size,
        resumable: leg.probe.resumable,
        part_path: leg.part_path,
        connections: leg.connections,
        min_split: leg.settings.min_split(),
        retries: leg.settings.retries,
        progress_base: leg.progress_base,
        max_request: http_download::max_request_for(&url),
        limiter: leg.limiter.clone(),
        check: Default::default(),
    };
    http_download::run(leg.shared.clone(), job, leg.segs.clone(), leg.progress.clone(), leg.cancel.clone()).await?;
    Ok(leg.probe.total_size.unwrap_or_else(|| leg.progress.bytes.load(Ordering::Relaxed)))
}
