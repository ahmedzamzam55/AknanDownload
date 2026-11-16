use super::Manager;
use super::cleanup::inside;
use super::torrent_ops::{TORRENT_DRY, TORRENT_DRY_TEXT};
use crate::sync::LockOk;
use crate::config::Settings;
use crate::filename;
use crate::http::Progress;
use crate::http_download::Cancelled;
use crate::model::{DownloadId, DownloadRecord, Status};
use crate::torrent;
use crate::bt;
use anyhow::anyhow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

impl Manager {
    pub(super) async fn run_torrent(
        self: &Arc<Self>,
        id: DownloadId,
        rec: DownloadRecord,
        progress: Arc<Progress>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let _ = std::fs::create_dir_all(&rec.save_dir);
        let opts = torrent::AddOptions {
            dir: rec.save_dir.clone(),
            select: rec.torrent_select.clone(),
            download_limit_kb: rec.speed_limit_kb.unwrap_or(0),
            check: rec.torrent_recheck,
            first_last: rec.torrent_first_last,
            trackers: rec.torrent_trackers.clone(),
        };
        if rec.torrent_recheck {
            self.update(id, |x| x.torrent_recheck = false);
        }
        // files that were there before: never removed by us
        let existed: HashSet<PathBuf> = rec.torrent_data_paths().into_iter().filter(|p| p.exists()).collect();
        self.torrent_limits().await;
        let gid = tokio::select! {
            r = self.torrents.add(&rec.url, &opts) => r.map_err(|e| {
                // librqbit refuses names with ".." itself
                if format!("{e:#}").contains("path traversal") {
                    anyhow!("this torrent has file names that lead out of its folder: it was stopped")
                } else {
                    e
                }
            })?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        self.update(id, |x| {
            if x.status == Status::Connecting {
                x.status = Status::Downloading;
            }
        });
        let mut named = false;
        let mut listed = !rec.torrent_files.is_empty();
        // since when nobody is connected (to say so after a while)
        let mut alone_since = Some(Instant::now());
        let mut said_alone = false;
        // since when people are connected but none of them sends anything
        let mut dry_since: Option<Instant> = None;
        let mut said_dry = false;
        // what was asked last (files, trackers, first / last pieces)
        // (a magnet link's chosen files: given once its file list is here)
        let mut applied = (
            if torrent::is_magnet(&rec.url) && rec.torrent_select.is_some() { Some(String::new()) } else { rec.torrent_select.clone() },
            rec.torrent_trackers.clone(),
            rec.torrent_first_last,
        );
        let mut rounds = 0u64;
        let mut checked_paths = false;
        loop {
            rounds += 1;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(800)) => {}
                _ = cancel.cancelled() => {
                    self.torrents.remove(&gid).await;
                    return Err(Cancelled.into());
                }
            }
            if rounds.is_multiple_of(5) {
                self.torrent_limits().await;
            }
            let st = match self.torrents.status(&gid).await {
                Ok(st) => st,
                Err(e) => {
                    self.torrents.remove(&gid).await;
                    return Err(e.context("the torrent program stopped"));
                }
            };
            // the chosen files only (the total also counts the pieces they
            // share with files that were left out)
            let files = if st.total > 0 { self.torrents.files(&gid).await.unwrap_or_default() } else { Vec::new() };
            // a magnet link's files (known only now): every one inside the folder
            if !checked_paths && !files.is_empty() {
                checked_paths = true;
                if files.iter().any(|f| !f.path.as_os_str().is_empty() && !torrent::stays_inside(&f.path, &rec.save_dir)) {
                    self.torrents.remove(&gid).await;
                    anyhow::bail!("this torrent has file names that lead out of its folder: it was stopped");
                }
            }
            // the user changed its files, trackers or "first and last pieces"
            let wanted = {
                let inner = self.inner.lock_ok();
                inner.entries.get(&id).map(|e| (e.rec.torrent_select.clone(), e.rec.torrent_trackers.clone(), e.rec.torrent_first_last))
            };
            if let Some(w) = wanted.filter(|w| *w != applied && !files.is_empty()) {
                let live = bt::Live { select: w.0.clone(), trackers: w.1.clone(), first_last: w.2 };
                if self.torrents.change(&gid, &live).await.is_ok() {
                    applied = w;
                }
            }
            // what its window shows
            {
                let peers = if rounds % 3 == 1 { self.torrents.peers(&gid).await.ok() } else { None };
                let mut live = self.torrent_live.lock_ok();
                let l = live.entry(id).or_default();
                l.files = files.clone();
                l.trackers = st.trackers.clone();
                l.pieces = torrent::piece_runs(&st.bitfield, st.pieces, 200);
                l.uploaded = st.upload_total;
                if let Some(p) = peers {
                    l.peers = p;
                }
            }
            let chosen: Vec<&torrent::FileState> = files.iter().filter(|f| f.selected).collect();
            let (total, done) = if chosen.is_empty() {
                (st.total, st.done)
            } else {
                (chosen.iter().map(|f| f.size).sum(), chosen.iter().map(|f| f.done.min(f.size)).sum())
            };
            progress.bytes.store(done, Ordering::Relaxed);
            progress.connections.store(st.connections, Ordering::Relaxed);
            progress.seeders.store(st.seeders, Ordering::Relaxed);
            if st.connections > 0 {
                alone_since = None;
                if said_alone {
                    said_alone = false;
                    self.update(id, |x| x.error = None);
                }
            } else if done < total.max(1) {
                let since = *alone_since.get_or_insert_with(Instant::now);
                if !said_alone && since.elapsed() > Duration::from_secs(90) {
                    said_alone = true;
                    self.update(id, |x| x.error = Some("nobody shares this torrent right now".into()));
                }
            }
            // connected, nothing comes: those people don't have the pieces
            // (or don't give to us); after a while other people are looked
            // for, and the user is told why it waits
            let wanting = total > 0 && done < total && st.status == "active" && !st.seeding;
            if wanting && st.connections > 0 && st.speed == 0 {
                let since = *dry_since.get_or_insert_with(Instant::now);
                if since.elapsed() > TORRENT_DRY {
                    dry_since = Some(Instant::now());
                    if !said_dry {
                        said_dry = true;
                        self.update(id, |x| x.error = Some(TORRENT_DRY_TEXT.into()));
                    }
                    let _ = self.torrents.look_for_others(&gid).await;
                    continue;
                }
            } else if st.speed > 0 {
                dry_since = None;
                if said_dry {
                    said_dry = false;
                    self.update(id, |x| {
                        if x.error.as_deref() == Some(TORRENT_DRY_TEXT) {
                            x.error = None;
                        }
                    });
                }
            }
            if !listed && !chosen.is_empty() {
                // the files it writes, to clean up exactly those later
                let rel: Vec<String> = chosen
                    .iter()
                    .filter_map(|f| f.path.strip_prefix(&rec.save_dir).ok().map(|r| r.to_string_lossy().into_owned()))
                    .filter(|r| !r.is_empty())
                    .collect();
                if !rel.is_empty() {
                    listed = true;
                    self.update(id, move |x| x.torrent_files = rel);
                }
            }
            if total > 0 {
                let name = if named { None } else { st.name.clone().map(|n| filename::sanitize(&n)) };
                named = named || name.is_some();
                self.update(id, move |x| {
                    x.total_size = Some(total);
                    x.resumable = true;
                    if let Some(n) = name {
                        x.file_name = n;
                    }
                });
            }
            let finished = match st.status.as_str() {
                "complete" => true,
                "error" => {
                    let msg = st.error.unwrap_or_else(|| "the torrent could not be downloaded".into());
                    self.torrents.remove(&gid).await;
                    anyhow::bail!("{msg}");
                }
                "removed" => anyhow::bail!("the torrent was stopped"),
                // everything is here and it shares for a while: done for us
                _ => st.seeding && total > 0 && done >= total,
            };
            if finished {
                // the bits of the files that were left out (pieces are shared
                // between files): not wanted, and not the user's own; deleted
                // once the torrent lets go of them
                let leftovers: Vec<PathBuf> = files
                    .iter()
                    .filter(|f| !f.selected && inside(&f.path, &rec.save_dir) && f.path.is_file() && !existed.contains(&f.path))
                    .map(|f| f.path.clone())
                    .collect();
                // it shares for the time (or the ratio) of Options, then it
                // is forgotten (no sharing wanted: forgotten now)
                self.torrents.share_then_forget(&gid, settings.torrent_seed_minutes, settings.torrent_seed_ratio, leftovers);
                return Ok(());
            }
        }
    }
}
