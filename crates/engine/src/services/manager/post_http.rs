use super::{Manager};
use super::clip_job::song_title;
use crate::sync::LockOk;
use crate::filename;
use crate::http::Progress;
use crate::http_download::{self, Cancelled, HttpJob, SegTable};
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Step};
use crate::mux;
use crate::ytdlp;
use crate::segments;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    pub(super) async fn retry_packed_holes(
        &self,
        rec: &DownloadRecord,
        job: HttpJob,
        segs: &SegTable,
        progress: &Arc<Progress>,
        cancel: &CancellationToken,
        part: &Path,
    ) -> anyhow::Result<()> {
        // a packed file (RAR, ZIP, 7z...) never has long runs of empty
        // bytes: such a run is a part that never came (the file was
        // "damaged" while other programs got it whole). Those parts
        // are fetched once more; empty again, they are what the
        // server has.
        if let (true, Some(total)) = (rec.resumable && is_packed(&rec.file_name), rec.total_size) {
            let p2 = part.to_path_buf();
            let holes = tokio::task::spawn_blocking(move || find_holes(&p2)).await.unwrap_or_default();
            if !holes.is_empty() {
                let missing: u64 = holes.iter().map(|(a, b)| b - a).sum();
                eprintln!("{}: {} empty parts ({missing} bytes): fetched again", rec.file_name, holes.len());
                *segs.lock_ok() = segments_for_holes(total, &holes);
                progress.bytes.fetch_sub(missing.min(progress.bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
                http_download::run(self.shared.clone(), job, segs.clone(), progress.clone(), cancel.clone()).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn finish_http(
        self: &Arc<Self>,
        id: DownloadId,
        rec: &DownloadRecord,
        progress: &Arc<Progress>,
        cancel: &CancellationToken,
        part: &Path,
    ) -> anyhow::Result<()> {
        if let Some(fmt) = rec.sub_convert.as_deref() {
            self.apply_sub_convert(id, part, fmt, progress)?;
        }
        if rec.source_key.as_deref() == Some(ytdlp::MP3_KEY) {
            // "Audio · MP3": the sound is here, now make the MP3 file
            progress.assembling.store(true, Ordering::Relaxed);
            let (input, tmp) = (part.to_path_buf(), part.with_extension("mp3tmp"));
            let t2 = tmp.clone();
            let title = song_title(&rec.file_name);
            let kbps = ytdlp::mp3_kbps();
            // ffmpeg missing: fetched again first
            crate::tools::restore_ffmpeg(&self.client()).await;
            progress.set_step(Some(Step::ToMp3));
            let (p2, c2) = (progress.clone(), cancel.clone());
            let made = tokio::task::spawn_blocking(move || mux::to_mp3_watched(&input, &t2, kbps, Some(&title), &|f| p2.step_at(f), Some(&c2))).await?;
            if made.is_err() {
                progress.set_step(None);
            }
            made?;
            std::fs::rename(&tmp, &part)?;
        }
        if rec.kind == DownloadKind::Hls && filename::extension(&rec.file_name) == "ts" {
            if mux::find_ffmpeg().is_none() {
                crate::tools::restore_ffmpeg(&self.client()).await;
            }
            if mux::find_ffmpeg().is_some() {
                // a joined stream: an ordinary MP4 plays everywhere and can be
                // wound forward; if that fails the .ts file stays as it is
                progress.assembling.store(true, Ordering::Relaxed);
                let (input, tmp) = (part.to_path_buf(), part.with_extension("mp4tmp"));
                let t2 = tmp.clone();
                progress.set_step(Some(Step::ToMp4));
                let (p2, c2) = (progress.clone(), cancel.clone());
                let r = tokio::task::spawn_blocking(move || mux::ts_to_mp4(&input, &t2, &|f| p2.step_at(f), Some(&c2))).await;
                if cancel.is_cancelled() {
                    progress.set_step(None);
                    return Err(Cancelled.into());
                }
                let made = r.map(|r| r.is_ok()).unwrap_or(false);
                if made && std::fs::rename(&tmp, &part).is_ok() {
                    let (stem, _) = filename::split_ext(&rec.file_name);
                    let name = filename::unique_name(&rec.save_dir, &format!("{stem}.mp4"));
                    self.update(id, move |x| x.file_name = name);
                } else {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
        }
        let latest = self.inner.lock_ok().entries.get(&id).map(|e| e.rec.clone()).unwrap_or_else(|| rec.clone());
        self.process_video_subtitle(id, &latest, progress, cancel, part).await?;
        self.finish_file(id, &latest, progress, cancel).await?;
        self.save_video_sidecar_subtitle(id, &latest).await;
        Ok(())
    }
}

/// Packed files: their bytes look random, long runs of zero bytes never
/// happen in them.
pub(super) fn is_packed(file_name: &str) -> bool {
    matches!(
        filename::extension(file_name).as_str(),
        "rar" | "zip" | "7z" | "gz" | "tgz" | "xz" | "txz" | "bz2" | "zst" | "lz" | "lzma" | "cab"
    ) || file_name.to_ascii_lowercase().contains(".part") && file_name.to_ascii_lowercase().ends_with(".rar")
}

/// Runs of at least 64 KiB of zero bytes in the file: [start, end).
pub(super) fn find_holes(path: &Path) -> Vec<(u64, u64)> {
    const MIN_HOLE: u64 = 64 * 1024;
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let mut buf = vec![0u8; 1 << 20];
    let (mut at, mut run_start): (u64, Option<u64>) = (0, None);
    let mut holes = Vec::new();
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let chunk = &buf[..n];
        // a whole buffer of zeros is checked fast
        if chunk.iter().all(|b| *b == 0) {
            run_start.get_or_insert(at);
        } else {
            for (i, b) in chunk.iter().enumerate() {
                let pos = at + i as u64;
                if *b == 0 {
                    run_start.get_or_insert(pos);
                } else if let Some(s) = run_start.take() {
                    if pos - s >= MIN_HOLE {
                        holes.push((s, pos));
                    }
                }
            }
        }
        at += n as u64;
    }
    if let Some(s) = run_start {
        if at - s >= MIN_HOLE {
            holes.push((s, at));
        }
    }
    holes
}

/// A table where only the holes are left to get.
pub(super) fn segments_for_holes(total: u64, holes: &[(u64, u64)]) -> Vec<segments::Segment> {
    let mut out = Vec::new();
    let mut at = 0;
    for &(a, b) in holes {
        if a > at {
            out.push(segments::Segment { pos: a, ..segments::Segment::new(at, a) });
        }
        out.push(segments::Segment::new(a, b));
        at = b;
    }
    if at < total {
        out.push(segments::Segment { pos: total, ..segments::Segment::new(at, total) });
    }
    out
}

