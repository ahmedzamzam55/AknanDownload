use super::learned::retry_after_of;
use super::types::{
    parse_content_range, Again, FileChanged, Leave, HEADERS_TIMEOUT, OVERLAP, SLOW_BYTES_PER_SEC, SLOW_SHARE,
    SLOW_WINDOW, STALL_TIMEOUT,
};
use super::worker::Worker;
use crate::fileio::write_at;
use crate::http::{request, ConnGuard, HttpStatus, NetDrop, SendOrdered};
use crate::segments;
use crate::sync::LockOk;
use anyhow::{anyhow, bail};
use reqwest::header::RANGE;
use reqwest::StatusCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Worker {
    pub(super) async fn fetch_once(&self, idx: usize) -> anyhow::Result<()> {
        let (start, pos, end) = {
            let table = self.segs.lock_ok();
            (table[idx].start, table[idx].pos, table[idx].end)
        };
        if pos >= end {
            return Ok(());
        }
        // where this single request stops (the segment may need several)
        let req_end = match self.job.max_request {
            Some(limit) if self.segmented => end.min(pos + limit.max(1)),
            _ => end,
        };
        // the first part of a new download: from the answer kept since the look
        let opened = if pos == 0 { self.job.check.opening.lock_ok().take() } else { None };
        let was_opened = opened.is_some();
        // going on inside a part: the last bytes before are asked again and
        // compared with what is on the disk
        let back = if self.segmented && !was_opened { pos.saturating_sub(start).min(OVERLAP) } else { 0 };
        let version = self.job.check.version.lock_ok().clone();
        let mut resp = match opened {
            Some(r) => r,
            None => {
                let mut rb = request(&crate::http::client_for(&self.shared.client(), &self.job.url), &self.job.url, &self.job.headers, &self.job.user_agent);
                if self.segmented {
                    rb = rb.header(RANGE, format!("bytes={}-{}", pos - back, req_end - 1));
                    if let Some(v) = &version {
                        rb = rb.header(reqwest::header::IF_RANGE, v.as_str());
                    }
                }
                // the answer (its headers) must come: a server that keeps quiet does
                // not hold this connection for ever
                match tokio::time::timeout(HEADERS_TIMEOUT, rb.send_ordered()).await {
                    Ok(r) => r?,
                    Err(_) => return Err(NetDrop("the server did not answer (timed out)").into()),
                }
            }
        };
        let status = resp.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.retry_after.store(retry_after_of(&resp).unwrap_or(0), Ordering::Relaxed);
        }
        if crate::http::is_bot_check(&resp) {
            return Err(anyhow!(HttpStatus(403)).context(crate::http::CAPTCHA));
        }
        if status == StatusCode::FORBIDDEN {
            // a site that wants a "not a robot" check (pixeldrain and others
            // say so in the answer): no new link or try helps, the browser must
            let body = tokio::time::timeout(Duration::from_secs(3), resp.chunk()).await.ok().and_then(|c| c.ok()).flatten();
            if body.is_some_and(|b| String::from_utf8_lossy(&b[..b.len().min(4096)]).to_ascii_lowercase().contains("captcha")) {
                return Err(anyhow!(HttpStatus(403)).context(crate::http::CAPTCHA));
            }
            return Err(HttpStatus(403).into());
        }
        if self.segmented {
            if status != StatusCode::PARTIAL_CONTENT {
                if status.is_client_error() || status.is_server_error() {
                    return Err(HttpStatus(status.as_u16()).into());
                }
                // asked "only if it is still this version": the whole file
                // came back, so it is another version - or the same file
                // under another name on another server of the same site
                if status == StatusCode::OK && version.is_some() {
                    let theirs = resp.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string());
                    let same_size = resp.content_length().is_some() && resp.content_length() == self.job.total_size;
                    if same_size && theirs != version {
                        // each server of the site names it its own way: asked
                        // again without the name (the bytes before are still compared)
                        *self.job.check.version.lock_ok() = None;
                        return Err(Again("asked again without the version check").into());
                    }
                    if !same_size {
                        return Err(FileChanged.into());
                    }
                }
                bail!("server did not accept the byte range (HTTP {})", status.as_u16());
            }
            // the part must be the one asked for (and of the same file):
            // anything else is never written
            if let Some((start, total)) = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range)
            {
                if start != pos - back {
                    return Err(NetDrop("the server sent another part of the file").into());
                }
                if let (Some(total), Some(known)) = (total, self.job.total_size) {
                    if total != known {
                        bail!("the link now gives another file: not the same file ({total} bytes instead of {known})");
                    }
                }
            }
        } else if !status.is_success() {
            return Err(HttpStatus(status.as_u16()).into());
        }
        let _guard = ConnGuard::new(&self.progress);
        // how fast this connection is compared with the whole download
        let mut window = std::time::Instant::now();
        let (mut mine, mut all_before) = (0u64, self.progress.bytes.load(Ordering::Relaxed));
        let mut slow_windows = 0u32;
        // bytes before `pos` still to compare with the disk
        let mut compare = back;

        loop {
            let chunk = match tokio::time::timeout(STALL_TIMEOUT, resp.chunk()).await {
                Ok(r) => r?,
                Err(_) => return Err(NetDrop("connection timed out").into()),
            };
            let Some(mut chunk) = chunk else { break };
            if compare > 0 {
                let k = compare.min(chunk.len() as u64);
                let at = pos - compare;
                let mut disk = vec![0u8; k as usize];
                crate::fileio::read_at(&self.file, &mut disk, at)?;
                if disk[..] != chunk[..k as usize] {
                    return Err(FileChanged.into());
                }
                compare -= k;
                chunk = chunk.slice(k as usize..);
                if chunk.is_empty() {
                    continue;
                }
            }
            if self.segmented && self.should_leave() {
                // fewer connections wanted: this one goes, its part stays for the others
                return Err(Leave.into());
            }
            mine += chunk.len() as u64;
            let spent = window.elapsed();
            if self.segmented && spent >= SLOW_WINDOW {
                let all_now = self.progress.bytes.load(Ordering::Relaxed);
                let conns = self.progress.connections.load(Ordering::Relaxed).max(1) as u64;
                let secs = spent.as_secs().max(1);
                let my_rate = mine / secs;
                let average = all_now.saturating_sub(all_before) / secs / conns;
                // much slower than the others (and the limiter is not the reason)
                let limited = self.shared.limiter.rate() > 0 || self.job.limiter.rate() > 0;
                // alone (the others are done): held to the best the
                // connections brought before (a slow last connection kept
                // the download at 90-99 % for ever)
                let best = self.crew.lock_ok().best;
                // (only near the end: a slow line is not made again and again)
                let near_end = self.job.total_size.is_some_and(|t| segments::downloaded(&self.segs.lock_ok()) >= t / 10 * 8);
                let compare = if conns >= 2 { average } else if near_end && self.job.connections >= 2 { best } else { u64::MAX };
                if !limited && my_rate < SLOW_BYTES_PER_SEC && my_rate.saturating_mul(SLOW_SHARE) < compare && compare != u64::MAX {
                    slow_windows += 1;
                } else {
                    slow_windows = 0;
                }
                if slow_windows >= 2 {
                    return Err(NetDrop("slow connection, made again").into());
                }
                window = std::time::Instant::now();
                mine = 0;
                all_before = all_now;
            }
            self.shared.limiter.acquire(chunk.len()).await;
            self.job.limiter.acquire(chunk.len()).await;
            let finished = {
                let mut table = self.segs.lock_ok();
                let s = &mut table[idx];
                // `end` may have moved down because another worker split us
                let stop = s.end.min(req_end);
                let room = stop.saturating_sub(s.pos);
                let take = (chunk.len() as u64).min(room) as usize;
                if take > 0 {
                    write_at(&self.file, &chunk[..take], s.pos)?;
                    s.pos += take as u64;
                    self.progress.bytes.fetch_add(take as u64, Ordering::Relaxed);
                    self.progress.online();
                }
                s.pos >= stop
            };
            if finished {
                return Ok(()); // dropping `resp` closes the connection
            }
        }

        // the server closed the stream
        let mut table = self.segs.lock_ok();
        let s = &mut table[idx];
        if !self.segmented && (self.job.total_size.is_none() || s.end == u64::MAX) {
            s.end = s.pos; // unknown size: end of stream means done
            return Ok(());
        }
        if s.pos >= s.end.min(req_end) {
            Ok(())
        } else {
            Err(NetDrop("connection closed early").into())
        }
    }
}
