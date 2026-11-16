use super::Manager;
use super::init::same_file;
use super::probe_ops::with_login;
use super::resolve::free_name;
use crate::sync::LockOk;
use crate::hls::{self, HlsJob};
use crate::http::{self};
use crate::http_download::{self, Cancelled, HttpJob};
use crate::model::{DownloadId, DownloadKind, Status};
use crate::segments;
use crate::system;
use anyhow::anyhow;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    pub(super) async fn run_download(self: &Arc<Self>, id: DownloadId, cancel: CancellationToken) -> anyhow::Result<()> {
        let settings = self.settings();
        let (mut rec, segs, audio_segs, progress, limiter) = {
            let inner = self.inner.lock_ok();
            let e = inner.entries.get(&id).ok_or_else(|| anyhow!("download was removed"))?;
            (e.rec.clone(), e.segs.clone(), e.audio_segs.clone(), e.progress.clone(), e.limiter.clone())
        };
        if let Some(q) = rec.pick_quality.clone() {
            self.resolve_pick(id, &mut rec, &q, &cancel).await?;
        }
        rec.headers = with_login(&settings, &rec.url, &rec.headers);
        rec.connections = settings.connections_for(&rec.url, rec.connections);
        if rec.clip.is_some() && rec.kind != DownloadKind::Torrent {
            return self.run_clip(id, rec, progress, limiter, cancel, &settings).await;
        }
        if rec.kind == DownloadKind::Dash {
            return self.run_dash(id, rec, segs, audio_segs, progress, limiter, cancel, &settings).await;
        }
        if rec.kind == DownloadKind::Torrent {
            return self.run_torrent(id, rec, progress, cancel, &settings).await;
        }
        if crate::ftp::is_ftp(&rec.url) {
            return self.run_ftp(id, rec, segs, progress, limiter, cancel, &settings).await;
        }
        self.refresh_source(id, &mut rec, &cancel).await?;

        // Fresh look at the server every time: links behind redirects often
        // expire, and the file may have changed since we paused.
        let client = self.shared.client();
        // nothing on the disk yet: the answer the look got a moment ago (kept
        // open) is where the file comes from, not a new request
        let fresh = rec.downloaded == 0 && segs.lock_ok().is_empty() && !self.relink.lock_ok().contains(&id);
        let kept = if fresh { http::take_kept(&rec.url) } else { None };
        if kept.is_none() {
            http::drop_kept(&rec.url);
        }
        let (p, mut opening) = match kept {
            Some((p, answer)) => (p, Some(answer)),
            None => {
                let p = tokio::select! {
                    r = http::probe(&client, &rec.url, &rec.headers, &settings.user_agent) => r?,
                    _ = cancel.cancelled() => return Err(Cancelled.into()),
                };
                // the look itself kept its answer: taken too
                let answer = if fresh { http::take_kept(&rec.url).map(|(_, a)| a) } else { http::drop_kept(&rec.url); None };
                (p, answer)
            }
        };
        if p.is_web_page() && rec.downloaded == 0 && segs.lock_ok().is_empty() {
            anyhow::bail!(
                "the address opens a web page, not a file (the link may have expired - use \"Refresh download address\")"
            );
        }
        // a new link after the old one stopped working: only the same file
        // (same size, parts can be asked for) goes on from where it was
        if self.relink.lock_ok().contains(&id) && !segs.lock_ok().is_empty() {
            same_file(&p, rec.total_size)?;
            self.relink.lock_ok().remove(&id);
        }
        {
            let mut table = segs.lock_ok();
            let changed = rec.total_size.is_some() && p.total_size != rec.total_size;
            // the same address now gives another version of the file (its
            // "ETag" or date changed): the parts already there are of the
            // old one, so it starts again rather than mixing the two
            let same_place = rec.final_url.as_deref() == Some(p.final_url.as_str());
            let other_version = !table.is_empty()
                && same_place
                && ((rec.etag.is_some() && p.etag.is_some() && rec.etag != p.etag)
                    || (rec.etag.is_none() && rec.server_time.is_some() && p.last_modified.is_some() && rec.server_time != p.last_modified));
            if changed || other_version || !p.resumable {
                table.clear();
            }
        }
        rec.etag = p.etag.clone().or(rec.etag.take());
        if rec.checksum.is_none() {
            rec.checksum = p.digest.as_deref().and_then(crate::checksum::Expected::parse).map(|e| e.text());
        }
        // the server only took the request without the browser's cookies /
        // referrer: the download asks the same way
        if let Some(h) = &p.headers_used {
            let auth = rec.headers.auth.clone();
            rec.headers = h.clone();
            rec.headers.auth = auth;
        }
        rec.final_url = Some(p.final_url.clone());
        rec.total_size = p.total_size;
        rec.resumable = p.resumable;
        rec.kind = p.kind;
        rec.server_time = p.last_modified.or(rec.server_time);
        rec.connections = settings.connections_for(&p.final_url, rec.connections);
        // where it was to go when this start began (the user may choose
        // another name / folder meanwhile, in "Download File Info")
        let (was_dir, was_name) = (rec.save_dir.clone(), rec.file_name.clone());
        // added without a name (batch, clipboard): use the server's real name
        // and the matching category folder before the first byte is written
        if rec.auto_name && segs.lock_ok().is_empty() && rec.downloaded == 0 && p.file_name != rec.file_name {
            let new_cat = settings.category_of(&p.file_name, &rec.url);
            let dir = if rec.save_dir == settings.dir_for(&rec.category) {
                settings.dir_for(&new_cat)
            } else {
                rec.save_dir.clone()
            };
            let _ = std::fs::create_dir_all(&dir);
            let name = free_name(&self.inner.lock_ok(), &dir, &p.file_name);
            rec.file_name = name;
            rec.save_dir = dir;
            rec.category = new_cat;
        }
        rec.auto_name = false;
        {
            let r = rec.clone();
            self.update(id, move |x| {
                x.final_url = r.final_url;
                x.total_size = r.total_size;
                x.resumable = r.resumable;
                x.kind = r.kind;
                // only when nobody changed them in the meantime
                if x.save_dir == was_dir && x.file_name == was_name {
                    x.file_name = r.file_name;
                    x.save_dir = r.save_dir;
                    x.category = r.category;
                }
                x.auto_name = false;
                x.server_time = r.server_time;
                x.etag = r.etag;
                if x.checksum.is_none() {
                    x.checksum = r.checksum;
                }
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }
        if let Some(total) = rec.total_size {
            let have = segments::downloaded(&segs.lock_ok());
            system::check_room(&rec.work_dir(), total.saturating_sub(have), Some(total)).map_err(|m| anyhow!(m))?;
            if rec.work_dir() != rec.save_dir {
                // the finished file is copied over to the other disk
                system::check_room(&rec.save_dir, total, Some(total)).map_err(|m| anyhow!(m))?;
            }
        }

        let part = rec.part_path();
        match rec.kind {
            DownloadKind::Http => {
                let job = HttpJob {
                    url: rec.request_url().to_string(),
                    headers: rec.headers.clone(),
                    user_agent: settings.user_agent.clone(),
                    total_size: rec.total_size,
                    resumable: rec.resumable,
                    part_path: part.clone(),
                    connections: rec.connections,
                    min_split: settings.min_split(),
                    retries: settings.retries,
                    progress_base: 0,
                    max_request: http_download::max_request_for(rec.request_url()),
                    limiter: limiter.clone(),
                    check: http_download::FileCheck::new(rec.etag.clone(), opening.take()),
                };
                http_download::run(self.shared.clone(), job.clone(), segs.clone(), progress.clone(), cancel.clone()).await?;
                self.retry_packed_holes(&rec, job, &segs, &progress, &cancel, &part).await?;
            }
            DownloadKind::Hls => {
                let job = HlsJob {
                    url: rec.request_url().to_string(),
                    headers: rec.headers.clone(),
                    user_agent: settings.user_agent.clone(),
                    output: part.clone(),
                    temp_dir: rec.hls_temp_dir(),
                    connections: rec.connections,
                    retries: settings.retries,
                    limiter: limiter.clone(),
                    clip: None,
                };
                hls::run(self.shared.clone(), job, progress.clone(), cancel.clone()).await?;
            }
            DownloadKind::Dash => unreachable!("handled by run_dash"),
            DownloadKind::Torrent => unreachable!("handled by run_torrent"),
        }
        self.finish_http(id, &rec, &progress, &cancel, &part).await
    }
}
