use super::{Manager};
use super::clip_cut::{cut_source, ClipSource, ClipWork};
use crate::config::Settings;
use crate::clip;
use crate::filename;
use crate::http::Progress;
use crate::limiter::SpeedLimiter;
use crate::model::{Clip, DownloadId, DownloadKind, DownloadRecord, Status};
use crate::ytdlp;
use anyhow::anyhow;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// The name an MP3 file shows: its file name without the quality part.
pub(super) fn song_title(file_name: &str) -> String {
    let (stem, _) = filename::split_ext(file_name);
    let t = stem.trim();
    match t.rfind(" [") {
        Some(i) if t.ends_with(']') => t[..i].trim().to_string(),
        _ => t.to_string(),
    }
}

impl Manager {
    /// "Download a part of a video": only the part is fetched (the parts of
    /// a stream that play then, or what ffmpeg reads of the video through
    /// our own address), then cut on the time asked.
    pub(super) async fn run_clip(
        self: &Arc<Self>,
        id: DownloadId,
        mut rec: DownloadRecord,
        progress: Arc<Progress>,
        limiter: Arc<SpeedLimiter>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let c = rec.clip.ok_or_else(|| anyhow!("no part chosen"))?;
        self.update(id, |x| {
            if x.status == Status::Connecting {
                x.status = Status::Downloading;
            }
        });
        let mp3 = (rec.source_key.as_deref() == Some(ytdlp::MP3_KEY)).then(|| (ytdlp::mp3_kbps(), song_title(&rec.file_name)));
        let out = clip::Output::for_name(&rec.file_name, mp3);
        let part = rec.part_path();
        let _ = std::fs::create_dir_all(rec.work_dir());
        if rec.kind != DownloadKind::Hls {
            // old links of a video page are asked for again
            self.refresh_source(id, &mut rec, &cancel).await?;
        }
        let src = ClipSource {
            url: rec.request_url().to_string(),
            audio_url: rec.audio_url.clone(),
            headers: rec.headers.clone(),
            hls: rec.kind == DownloadKind::Hls,
        };
        let work = ClipWork {
            joined: rec.video_part_path(),
            parts: rec.hls_temp_dir(),
            connections: rec.connections,
            retries: settings.retries,
            user_agent: settings.user_agent.clone(),
        };
        let result = cut_source(self.shared.clone(), &src, c, &out, &part, &work, limiter, progress.clone(), &cancel).await;
        let done = result?;
        if done == clip::Done::QuickInstead {
            self.update(id, |x| {
                let note = "The exact cut is not possible for this video's kind of picture: it was cut the quick way.";
                x.description = Some(match x.description.take().filter(|d| !d.trim().is_empty()) {
                    Some(d) => format!("{d}\n{note}"),
                    None => note.to_string(),
                });
            });
        }
        progress.assembling.store(false, Ordering::Relaxed);
        self.finish_file(id, &rec, &progress, &cancel).await
    }


    /// "Preview" of a part before it is downloaded: the part, cut the quick
    /// way into a file of its own (only the part is fetched), for a player.
    /// `then` gets the file. The returned token stops it.
    pub fn preview_part(
        self: &Arc<Self>,
        src: ClipSource,
        c: Clip,
        sound_only: bool,
        then: impl FnOnce(Result<PathBuf, String>) + Send + 'static,
    ) -> CancellationToken {
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let me = self.clone();
        self.rt.spawn(async move {
            let dir = me.data_dir.join("preview");
            // one preview at a time: the last ones go
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let (out, ext) = if sound_only { (clip::Output::Mp3(128, None), "mp3") } else { (clip::Output::Video, "mp4") };
            let output = dir.join(format!("part-{n}.{ext}"));
            let settings = me.settings();
            let work = ClipWork {
                joined: dir.join("joined.tmp"),
                parts: dir.join("parts"),
                connections: settings.connections().clamp(1, 8),
                retries: settings.retries,
                user_agent: settings.user_agent.clone(),
            };
            let quick = Clip { exact: false, ..c };
            let r = cut_source(
                me.shared.clone(),
                &src,
                quick,
                &out,
                &output,
                &work,
                Arc::new(SpeedLimiter::new(0)),
                Arc::new(Progress::default()),
                &cancel,
            )
            .await;
            let _ = std::fs::remove_dir_all(dir.join("parts"));
            then(r.map(|_| output).map_err(|e| format!("{e:#}")));
        });
        token
    }
}
