use super::learned::{learn, lower_wanted};
use super::types::{Again, Cancelled, FileChanged, Leave};
use super::worker::Worker;
use crate::http::{http_status_of, is_network_error, reconnect_delay};
use crate::sync::LockOk;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Worker {
    /// Download one segment, retrying with back-off and continuing from where
    /// the previous attempt stopped.
    pub(super) async fn download_segment(&self, idx: usize) -> anyhow::Result<()> {
        let mut attempt = 0u32;
        let mut net_tries = 0u32;
        let mut too_many = 0u32;
        loop {
            if self.cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            let before = self.segs.lock_ok()[idx].pos;
            let outcome = tokio::select! {
                r = self.fetch_once(idx) => r,
                _ = self.cancel.cancelled() => return Err(Cancelled.into()),
            };
            match outcome {
                Ok(()) => {
                    if self.segs.lock_ok()[idx].is_finished() {
                        return Ok(());
                    }
                    attempt = 0; // one request done, ask for the next piece
                }
                Err(e) if e.is::<Leave>() => return Err(e),
                Err(e) if e.is::<Again>() && attempt < 3 => attempt += 1,
                // another file now: no try brings this one back
                Err(e) if e.is::<FileChanged>() => return Err(e),
                Err(e) => {
                    let after = self.segs.lock_ok()[idx].pos;
                    if after > before {
                        attempt = 0; // it was making progress, don't give up yet
                        net_tries = 0;
                    }
                    let network = is_network_error(&e);
                    let status = http_status_of(&e);
                    if status == Some(429) {
                        // "too many requests": the site allows fewer connections
                        // at a time. One less from now on (this one goes, its
                        // part stays for the others), and the site's next
                        // downloads start with that many; the last ones wait
                        // as long as the site asks and go on.
                        if self.segmented {
                            let now = {
                                let c = self.crew.lock_ok();
                                c.running.saturating_sub(c.leaving)
                            };
                            let fewer = now.saturating_sub(1).max(1);
                            lower_wanted(&self.progress, self.job.connections.max(1), fewer);
                            learn(&self.job.url, fewer);
                            if self.should_leave() {
                                return Err(Leave.into());
                            }
                        }
                        too_many += 1;
                        if too_many > 40 {
                            return Err(e);
                        }
                        let asked = self.retry_after.swap(0, Ordering::Relaxed);
                        let wait = if asked > 0 { asked } else { (3 * too_many as u64).min(60) };
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(wait)) => {},
                            _ = self.cancel.cancelled() => return Err(Cancelled.into()),
                        }
                        continue;
                    }
                    if matches!(status, Some(401 | 403)) && self.segmented && !crate::http::wants_captcha(&e) && self.others_bring_data().await {
                        // refused while the other connections of the same
                        // link go on: not an expired link but one connection
                        // too many (pixeldrain and other sites answer 403, not
                        // 429). One less from now on, as for 429; this one
                        // goes and its part stays for the others
                        let now = {
                            let c = self.crew.lock_ok();
                            c.running.saturating_sub(c.leaving)
                        };
                        let fewer = now.saturating_sub(1).max(1);
                        lower_wanted(&self.progress, self.job.connections.max(1), fewer);
                        learn(&self.job.url, fewer);
                        if self.should_leave() {
                            return Err(Leave.into());
                        }
                        too_many += 1;
                        if too_many > 40 {
                            return Err(e);
                        }
                        continue;
                    }
                    if network && self.segmented && !format!("{e:#}").contains("slow connection") && self.server_drops_too_many() {
                        // the server keeps dropping connections while the
                        // others go on (sites that allow only a few): half as
                        // many from now on; this one goes, its part stays
                        if self.should_leave() {
                            return Err(Leave.into());
                        }
                    }
                    if network {
                        // no internet / reset / stall: wait as long as it takes,
                        // the part keeps what it already has
                        net_tries += 1;
                        self.progress.waiting.store(true, Ordering::Relaxed);
                    } else {
                        attempt += 1;
                        // an expired or removed link won't come back by itself
                        let gone = matches!(status, Some(401 | 403 | 404 | 410));
                        let limit = if gone { self.job.retries.min(2) } else { self.job.retries };
                        if attempt > limit {
                            if gone {
                                return Err(e.context(
                                    "the link has expired or the file was removed - use \"Refresh download address\"",
                                ));
                            }
                            return Err(e);
                        }
                    }
                    if !self.segmented {
                        // can't resume without ranges: restart from zero
                        let mut table = self.segs.lock_ok();
                        let s = &mut table[idx];
                        self.progress.bytes.fetch_sub(s.done(), Ordering::Relaxed);
                        s.pos = s.start;
                        let _ = self.file.set_len(0);
                    }
                    let made_again = format!("{e:#}").contains("slow connection");
                    let backoff = if made_again {
                        // a slow connection made again: at once
                        Duration::ZERO
                    } else if network {
                        reconnect_delay(net_tries)
                    } else {
                        Duration::from_secs((1u64 << attempt.min(5)).min(30))
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {},
                        // another part got through: the internet is back
                        _ = self.progress.back_online.notified() => {},
                        _ = self.cancel.cancelled() => return Err(Cancelled.into()),
                    }
                }
            }
        }
    }
}
