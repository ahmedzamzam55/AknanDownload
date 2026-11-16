use super::{Manager};
use super::init::same_file;
use crate::sync::LockOk;
use crate::config::Settings;
use crate::http::{self, Progress};
use crate::http_download::{self, Cancelled, HttpJob, SegTable};
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadId, DownloadRecord, Status, Step};
use crate::mux;
use crate::segments;
use crate::system;
use anyhow::anyhow;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    /// many connections, then merged by ffmpeg into one file.
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
        let audio_url = rec
            .audio_url
            .clone()
            .ok_or_else(|| anyhow!("this download has no audio stream"))?;
        let finished = |t: &SegTable| {
            let t = t.lock_ok();
            !t.is_empty() && segments::all_finished(&t)
        };
        // both parts already on disk (e.g. ffmpeg was missing last time):
        // skip the network completely, the links may have expired by now
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
                if !segs.lock_ok().is_empty() {
                    same_file(&vp, rec.video_size)?;
                }
                if !audio_segs.lock_ok().is_empty() {
                    same_file(&ap, old_audio)?;
                }
                self.relink.lock_ok().remove(&id);
            }
            if vp.total_size != rec.video_size || !vp.resumable {
                segs.lock_ok().clear();
            }
            if ap.total_size != old_audio || !ap.resumable {
                audio_segs.lock_ok().clear();
            }
            rec.video_size = vp.total_size;
            rec.total_size = vp.total_size.zip(ap.total_size).map(|(v, a)| v + a);
            rec.resumable = vp.resumable && ap.resumable;
            rec.final_url = Some(vp.final_url.clone());
            if let Some(total) = rec.total_size {
                let have = segments::downloaded(&segs.lock_ok()) + segments::downloaded(&audio_segs.lock_ok());
                // video + audio parts, then the merged file next to them
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

            let video_job = HttpJob {
                url: vp.final_url.clone(),
                headers: rec.headers.clone(),
                user_agent: ua.clone(),
                total_size: vp.total_size,
                resumable: vp.resumable,
                part_path: rec.video_part_path(),
                connections: rec.connections,
                min_split: settings.min_split(),
                retries: settings.retries,
                progress_base: 0,
                max_request: http_download::max_request_for(&vp.final_url),
                limiter: limiter.clone(),
                check: Default::default(),
            };
            http_download::run(self.shared.clone(), video_job.clone(), segs.clone(), progress.clone(), cancel.clone())
                .await?;
            let video_bytes = vp.total_size.unwrap_or_else(|| progress.bytes.load(Ordering::Relaxed));
            let audio_job = HttpJob {
                url: ap.final_url.clone(),
                total_size: ap.total_size,
                resumable: ap.resumable,
                part_path: rec.audio_part_path(),
                progress_base: video_bytes,
                max_request: http_download::max_request_for(&ap.final_url),
                ..video_job
            };
            http_download::run(self.shared.clone(), audio_job, audio_segs.clone(), progress.clone(), cancel.clone())
                .await?;
        } else {
            self.update(id, |x| {
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }

        // merge
        progress.assembling.store(true, Ordering::Relaxed);
        let (video, audio, output, name) = (
            rec.video_part_path(),
            rec.audio_part_path(),
            rec.part_path(),
            rec.file_name.clone(),
        );
        // ffmpeg missing: fetched again first
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
        self.finish_file(id, &rec, &progress, &cancel).await
    }

}
