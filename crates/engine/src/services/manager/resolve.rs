use super::{Inner, Manager};
use crate::sync::{LockOk, RwOk};
use crate::filename;
use crate::http_download::Cancelled;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Status};
use crate::ytdlp;
use anyhow::anyhow;
use chrono::Utc;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// The file name of a playlist video: the name the playlist gave it, or,
/// when it gave none ("video"), its own `title` after the "07 - " number.
pub(super) fn playlist_stem(current: &str, title: &str) -> String {
    let title = title.trim();
    let digits = current.chars().take_while(|c| c.is_ascii_digit()).count();
    let number = if digits > 0 && current[digits..].starts_with(" - ") { &current[..digits + 3] } else { "" };
    let named = current[number.len()..].trim();
    if title.is_empty() || !(named.is_empty() || named.eq_ignore_ascii_case("video")) {
        return current.to_string();
    }
    format!("{number}{title}")
}

/// A name that is free on disk and not used by another unfinished download.
pub(super) fn free_name(inner: &Inner, dir: &Path, wanted: &str) -> String {
    let used = |n: &str| {
        inner
            .entries
            .values()
            .any(|e| e.rec.status != Status::Completed && e.rec.save_dir == dir && e.rec.file_name == n)
    };
    let first = filename::unique_name(dir, wanted);
    if !used(&first) {
        return first;
    }
    let (stem, ext) = filename::split_ext(wanted);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        if !used(&candidate) && filename::unique_name(dir, &candidate) == candidate {
            return candidate;
        }
    }
    first
}

impl Manager {
    /// A playlist video added as its page: ask yt-dlp for its links now and
    /// take the wanted quality.
    pub(super) async fn resolve_pick(
        &self,
        id: DownloadId,
        rec: &mut DownloadRecord,
        quality: &str,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        let page = rec.source_page.clone().unwrap_or_else(|| rec.url.clone());
        let p2 = page.clone();
        let job = tokio::task::spawn_blocking(move || ytdlp::resolve(&p2, None));
        let info = tokio::select! {
            r = job => r??,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        let opt = ytdlp::pick_option(&info.options, quality)
            .ok_or_else(|| anyhow!("no downloadable video was found on this page"))?;
        rec.url = opt.video_url.clone();
        rec.final_url = None;
        rec.audio_url = opt.audio_url.clone();
        rec.kind = if opt.audio_url.is_some() { DownloadKind::Dash } else { DownloadKind::Http };
        rec.source_page = Some(page.clone());
        rec.source_key = Some(opt.key.clone());
        ytdlp::apply_headers(&mut rec.headers, &opt.headers);
        // The name shown in the playlist window stays (it is in the user's
        // language); only a list that gave no name takes the video's own
        // title, keeping the "07 - " number of the playlist. The real
        // container decides the extension.
        let (stem, ext) = filename::split_ext(&rec.file_name);
        let stem = playlist_stem(stem, &info.title);
        if !ext.eq_ignore_ascii_case(&opt.ext) || stem != filename::split_ext(&rec.file_name).0 {
            let wanted = filename::sanitize(&format!("{stem}.{}", opt.ext));
            rec.file_name = free_name(&self.inner.lock_ok(), &rec.save_dir, &wanted);
        }
        rec.pick_quality = None;
        rec.category = self.settings.read_ok().category_of(&rec.file_name, &page);
        let r = rec.clone();
        self.update(id, move |x| {
            x.url = r.url;
            x.final_url = None;
            x.audio_url = r.audio_url;
            x.kind = r.kind;
            x.source_page = r.source_page;
            x.source_key = r.source_key;
            x.headers = r.headers;
            x.file_name = r.file_name;
            x.category = r.category;
            x.pick_quality = None;
        });
        Ok(())
    }

    /// YouTube links expire after a few hours. When this download came from a
    /// YouTube page and is being resumed (or waited a while), ask yt-dlp for
    /// fresh links of the same quality. Failure is not fatal: we try the old ones.
    pub(super) async fn refresh_source(
        &self,
        id: DownloadId,
        rec: &mut DownloadRecord,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        let (Some(page), Some(key)) = (rec.source_page.clone(), rec.source_key.clone()) else {
            return Ok(());
        };
        let started = !rec.segments.is_empty() || rec.downloaded > 0;
        let old = Utc::now().signed_duration_since(rec.added_at) > chrono::Duration::minutes(30);
        if !started && !old {
            return Ok(());
        }
        let job = tokio::task::spawn_blocking(move || ytdlp::resolve_key(&page, &key));
        let fresh = tokio::select! {
            r = job => r,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        if let Ok(Ok(opt)) = fresh {
            rec.url = opt.video_url.clone();
            rec.final_url = None;
            rec.audio_url = opt.audio_url.clone();
            ytdlp::apply_headers(&mut rec.headers, &opt.headers);
            let r = rec.clone();
            self.update(id, move |x| {
                x.url = r.url;
                x.final_url = None;
                x.audio_url = r.audio_url;
                x.headers = r.headers;
            });
        }
        Ok(())
    }

}
