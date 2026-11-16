use super::pump::TRYING_AGAIN;
use super::{Event, Manager};
use crate::sync::LockOk;
use crate::config::AfterDone;
use crate::filename;
use crate::http::{self};
use crate::http_download::Cancelled;
use crate::model::{DownloadId, DownloadKind, Status};
use crate::ytdlp;
use crate::system;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    pub(super) async fn run_entry(self: Arc<Self>, id: DownloadId, cancel: CancellationToken) {
        let progress = self.inner.lock_ok().entries.get(&id).map(|e| e.progress.clone());
        let mut tries = 0u32;
        // server trouble: tries without anything new downloaded in between
        let mut stuck = 0u32;
        // new links fetched because the old one stopped working
        let mut relinks = 0u32;
        // the bytes when a new link was fetched last: a link that worked a
        // while again starts the count anew (a long download whose link
        // expires every few hours is not given up after 6)
        let mut relink_bytes = 0u64;
        // a download without resume started again from the beginning
        let mut restarts = 0u32;
        let mut last_bytes = progress.as_ref().map(|p| p.bytes.load(Ordering::Relaxed)).unwrap_or(0);
        let result = loop {
            let r = self.run_download(id, cancel.clone()).await;
            match (&r, &progress) {
                // no internet when starting (or the whole transfer broke off):
                // wait in place and try again until it is back or the user stops it
                (Err(e), Some(p)) if !cancel.is_cancelled() && !e.is::<Cancelled>() && http::is_network_error(e) => {
                    tries += 1;
                    p.waiting.store(true, Ordering::Relaxed);
                    tokio::select! {
                        _ = tokio::time::sleep(http::reconnect_delay(tries)) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                // a busy server, a transfer that broke off (big files): it
                // goes on by itself later, from where it was. Given up only
                // when many tries in a row brought nothing new (hours).
                // the link stopped working (expired): a new one, checked before
                // anything more is written; given up after 6 tries
                // (a download that got nothing yet: 2 tries, it may never have worked)
                (Err(e), Some(p))
                    if !cancel.is_cancelled()
                        && !e.is::<Cancelled>()
                        && http::is_link_gone(e)
                        && relinks < if p.bytes.load(Ordering::Relaxed) > 0 { 6 } else { 2 } =>
                {
                    let now = p.bytes.load(Ordering::Relaxed);
                    if relinks > 0 && now > relink_bytes {
                        relinks = 0;
                    }
                    relink_bytes = now;
                    relinks += 1;
                    self.relink.lock_ok().insert(id);
                    let wait = http::relink_delay(relinks);
                    let note = format!("the link stopped working - getting a new one in {} s (try {relinks})", wait.as_secs());
                    self.update(id, move |x| x.error = Some(note));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                (Err(e), Some(p)) if !cancel.is_cancelled() && !e.is::<Cancelled>() && http::is_passing_error(e) && self.resumable(id) => {
                    let now = p.bytes.load(Ordering::Relaxed);
                    stuck = if now > last_bytes { 1 } else { stuck + 1 };
                    last_bytes = now;
                    // about an hour of tries that brought nothing
                    if stuck > 60 {
                        break r;
                    }
                    let wait = http::retry_delay(stuck);
                    let note = format!("{e:#} - trying again in {} s", wait.as_secs());
                    self.update(id, move |x| x.error = Some(note));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                // a server that can't resume (no byte ranges) had trouble: it
                // starts again from the beginning by itself, a few times
                (Err(e), Some(_)) if !cancel.is_cancelled() && !e.is::<Cancelled>() && http::is_passing_error(e) && restarts < 3 => {
                    restarts += 1;
                    let wait = http::retry_delay(restarts);
                    let note = format!("{e:#} - the server can't resume, starting again in {} s (try {restarts})", wait.as_secs());
                    self.update(id, move |x| x.error = Some(note));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                _ => break r,
            }
        };
        if let Some(p) = &progress {
            p.waiting.store(false, Ordering::Relaxed);
        }
        self.relink.lock_ok().remove(&id);
        {
            let mut inner = self.inner.lock_ok();
            let mut finished = false;
            let mut failed = false;
            let mut open_file = false;
            let mut mark: Option<(PathBuf, String, Option<String>)> = None;
            let mut action = AfterDone::Nothing;
            let settings = self.settings();
            if let Some(e) = inner.entries.get_mut(&id) {
                e.task = None;
                e.rec.segments = e.segs.lock_ok().clone();
                e.rec.audio_segments = e.audio_segs.lock_ok().clone();
                e.rec.downloaded = e.progress.bytes.load(Ordering::Relaxed);
                e.rec.parts_total = e.progress.parts_total.load(Ordering::Relaxed);
                e.rec.parts_done = e.progress.parts_done.load(Ordering::Relaxed);
                e.progress.assembling.store(false, Ordering::Relaxed);
                match result {
                    Ok(()) => {
                        e.rec.status = Status::Completed;
                        e.rec.error = None;
                        e.rec.finished_at = Some(Utc::now());
                        e.rec.segments.clear();
                        e.rec.audio_segments.clear();
                        if e.rec.total_size.is_none() {
                            e.rec.total_size = Some(e.rec.downloaded);
                        }
                        e.segs.lock_ok().clear();
                        e.audio_segs.lock_ok().clear();
                        finished = true;
                        mark = Some((
                            e.rec.target_path(),
                            e.rec.final_url.clone().unwrap_or_else(|| e.rec.url.clone()),
                            e.rec.headers.referer.clone(),
                        ));
                        action = std::mem::take(&mut e.rec.on_complete);
                        open_file = e.rec.queue.and_then(|q| settings.queue(q)).map(|q| q.open_when_done).unwrap_or(false);
                    }
                    Err(err) if cancel.is_cancelled() || err.is::<Cancelled>() => {
                        if e.rec.status.is_running() {
                            e.rec.status = Status::Paused;
                        }
                    }
                    Err(err) => {
                        // the queue tries a failed file again a few times
                        let retries = e.rec.queue.and_then(|q| settings.queue(q)).map(|q| q.retries).unwrap_or(0);
                        if e.rec.retry_count < retries {
                            e.rec.retry_count += 1;
                            e.rec.status = Status::Queued;
                            e.rec.error = Some(format!("{err:#}{TRYING_AGAIN}"));
                        } else {
                            e.rec.status = Status::Failed;
                            e.rec.error = Some(format!("{err:#}"));
                            failed = true;
                        }
                    }
                }
            }
            if finished && settings.subtitles && !settings.subtitle_langs.trim().is_empty() {
                // a site's video: its subtitles next to it (when there are any)
                if let Some(e) = inner.entries.get(&id) {
                    if let (Some(page), DownloadKind::Dash | DownloadKind::Http | DownloadKind::Hls, None, None) = (
                        e.rec.source_page.clone(),
                        e.rec.kind,
                        e.rec.subtitle.as_ref(),
                        e.rec.sub_convert.as_ref(),
                    ) {
                        let (dir, langs) = (e.rec.save_dir.clone(), settings.subtitle_langs.clone());
                        let (stem, _) = filename::split_ext(&e.rec.file_name);
                        let stem = stem.to_string();
                        if !matches!(filename::extension(&e.rec.file_name).as_str(), "mp3" | "m4a" | "opus" | "ogg") {
                            self.rt.spawn_blocking(move || {
                                let _ = ytdlp::fetch_subtitles(&page, &dir, &stem, &langs);
                            });
                        }
                    }
                }
            }
            if finished {
                // before anything can open it: the file comes from the internet
                if let Some((path, host, referrer)) = &mark {
                    system::mark_from_internet(path, host, referrer.as_deref());
                }
                inner.completed.push(id);
                inner.events.push(Event::Completed(id));
                if open_file {
                    inner.events.push(Event::OpenFile(id));
                }
            }
            if failed {
                inner.events.push(Event::Failed(id));
            }
            inner.dirty = true;
            drop(inner);
            if action != AfterDone::Nothing {
                self.after_done(action);
            }
        }
        self.pump();
    }
}
