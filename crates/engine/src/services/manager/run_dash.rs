use super::init::same_file;
use super::run_dash_stream::{download_leg, DashLeg};
use super::Manager;
use crate::config::Settings;
use crate::http::{self, Progress};
use crate::http_download::{Cancelled, SegTable};
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Status, Step};
use crate::mux;
use crate::segments;
use crate::sync::LockOk;
use crate::system;
use anyhow::anyhow;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    /// Separate video and audio streams (HTTP or HLS): downloaded and merged by ffmpeg.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_dash(
        self: &Arc<Self>,
        id: DownloadId,
        mut rec: DownloadRecord,
        segs: SegTable,
        audio_segs: SegTable,
        progress: Arc<Progress>,
        limiter: Arc<SpeedLimiter>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let audio_url = rec.audio_url.clone().ok_or_else(|| anyhow!("this download has no audio stream"))?;
        let finished = |t: &SegTable| {
            let t = t.lock_ok();
            !t.is_empty() && segments::all_finished(&t)
        };
        let both_done = finished(&segs)
            && finished(&audio_segs)
            && rec.video_part_path().exists()
            && rec.audio_part_path().exists();

        if !both_done {
            self.refresh_source(id, &mut rec, &cancel).await?;
            let audio_url = rec.audio_url.clone().unwrap_or(audio_url);
            let ua = settings.user_agent.clone();
            let client = self.shared.client();
            let probes = async {
                tokio::try_join!(
                    http::probe(&client, &rec.url, &rec.headers, &ua),
                    http::probe(&client, &audio_url, &rec.headers, &ua),
                )
            };
            let (vp, ap) = tokio::select! {
                r = probes => r?,
                _ = cancel.cancelled() => return Err(Cancelled.into()),
            };
            let old_audio = rec.total_size.zip(rec.video_size).map(|(t, v)| t.saturating_sub(v));
            if self.relink.lock_ok().contains(&id) {
                if !segs.lock_ok().is_empty() && vp.kind != DownloadKind::Hls {
                    same_file(&vp, rec.video_size)?;
                }
                if !audio_segs.lock_ok().is_empty() && ap.kind != DownloadKind::Hls {
                    same_file(&ap, old_audio)?;
                }
                self.relink.lock_ok().remove(&id);
            }
            if vp.kind != DownloadKind::Hls && (vp.total_size != rec.video_size || !vp.resumable) {
                segs.lock_ok().clear();
            }
            if ap.kind != DownloadKind::Hls && (ap.total_size != old_audio || !ap.resumable) {
                audio_segs.lock_ok().clear();
            }
            rec.video_size = vp.total_size;
            rec.total_size = vp.total_size.zip(ap.total_size).map(|(v, a)| v + a);
            rec.resumable = vp.resumable && ap.resumable;
            rec.final_url = Some(vp.final_url.clone());
            if let Some(total) = rec.total_size {
                let have = segments::downloaded(&segs.lock_ok()) + segments::downloaded(&audio_segs.lock_ok());
                system::check_room(&rec.work_dir(), (total * 2).saturating_sub(have), Some(total)).map_err(|m| anyhow!(m))?;
            }
            {
                let r = rec.clone();
                self.update(id, move |x| {
                    x.final_url = r.final_url;
                    x.total_size = r.total_size;
                    x.video_size = r.video_size;
                    x.resumable = r.resumable;
                    if x.status == Status::Connecting {
                        x.status = Status::Downloading;
                    }
                });
            }

            let hls_base = rec.hls_temp_dir();
            let video_bytes = if finished(&segs) && rec.video_part_path().exists() {
                std::fs::metadata(rec.video_part_path()).map(|m| m.len()).unwrap_or(0)
            } else {
                download_leg(DashLeg {
                    shared: &self.shared,
                    probe: &vp,
                    fallback_url: &rec.url,
                    headers: &rec.headers,
                    part_path: rec.video_part_path(),
                    hls_dir: hls_base.join("v"),
                    segs: &segs,
                    connections: rec.connections,
                    progress_base: 0,
                    limiter: &limiter,
                    progress: &progress,
                    cancel: &cancel,
                    settings,
                })
                .await?
            };
            if !(finished(&audio_segs) && rec.audio_part_path().exists()) {
                download_leg(DashLeg {
                    shared: &self.shared,
                    probe: &ap,
                    fallback_url: &audio_url,
                    headers: &rec.headers,
                    part_path: rec.audio_part_path(),
                    hls_dir: hls_base.join("a"),
                    segs: &audio_segs,
                    connections: rec.connections,
                    progress_base: video_bytes,
                    limiter: &limiter,
                    progress: &progress,
                    cancel: &cancel,
                    settings,
                })
                .await?;
            }
        } else {
            self.update(id, |x| {
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }

        progress.assembling.store(true, Ordering::Relaxed);
        let (video, audio, output, name) = (rec.video_part_path(), rec.audio_part_path(), rec.part_path(), rec.file_name.clone());
        crate::tools::restore_ffmpeg(&self.client()).await;
        progress.set_step(Some(Step::Join));
        let (p2, c2) = (progress.clone(), cancel.clone());
        let joined = tokio::task::spawn_blocking(move || mux::merge(&video, &audio, &output, &name, &|f| p2.step_at(f), Some(&c2))).await?;
        if joined.is_err() {
            progress.set_step(None);
        }
        joined?;
        let _ = std::fs::remove_file(rec.video_part_path());
        let _ = std::fs::remove_file(rec.audio_part_path());
        let _ = std::fs::remove_dir_all(rec.hls_temp_dir());
        let part = rec.part_path();
        self.process_video_subtitle(id, &rec, &progress, &cancel, &part).await?;
        self.finish_file(id, &rec, &progress, &cancel).await?;
        self.save_video_sidecar_subtitle(id, &rec).await;
        Ok(())
    }
}
