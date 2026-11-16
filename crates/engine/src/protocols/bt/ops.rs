use super::{Live, Torrents};
use crate::sync::LockOk;
use crate::torrent::{is_magnet, is_tracker, parse_torrent, AddOptions};
use anyhow::{anyhow, bail, Context};
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, TorrentStatsState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

impl Torrents {
    /// Change what a running torrent gets. New trackers: the torrent starts
    /// again with them (nothing is checked again, nothing is lost).
    pub async fn change(&self, gid: &str, live: &Live) -> anyhow::Result<()> {
        let mut h = self.handle(gid).await?;
        let missing = live.trackers.iter().any(|t| is_tracker(t) && !h.shared().trackers.iter().any(|u| u.as_str() == t.trim()));
        if missing {
            let bytes = h.with_metadata(|m| m.torrent_bytes.clone())?;
            let o = AddOptions {
                dir: h.output_folder().to_path_buf(),
                download_limit_kb: self.own_limit.lock_ok().get(gid).copied().unwrap_or(0),
                trackers: live.trackers.clone(),
                ..Default::default()
            };
            h = self.start_again(gid, AddTorrent::from_bytes(bytes), &o).await?;
            // what is on the disk is taken from the notes: ready in a moment
            let _ = tokio::time::timeout(Duration::from_secs(30), h.wait_until_initialized()).await;
        }
        self.apply(gid, &h, live.select.as_deref(), live.first_last).await
    }

    /// The file list of a magnet link before it is downloaded: only the
    /// torrent's own description is fetched from the people who share it,
    /// into `dir` (a .torrent file, given back). Gives up after `wait`.
    pub async fn metadata(&self, magnet: &str, dir: &Path, wait: Duration) -> anyhow::Result<PathBuf> {
        if !is_magnet(magnet) {
            bail!("not a magnet link");
        }
        std::fs::create_dir_all(dir)?;
        let session = self.session().await?.clone();
        let opts = AddTorrentOptions { list_only: true, ..Default::default() };
        let r = tokio::time::timeout(wait, session.add_torrent(AddTorrent::from_url(magnet.trim().to_string()), Some(opts)))
            .await
            .map_err(|_| anyhow!("nobody sent the file list in time"))??;
        let AddTorrentResponse::ListOnly(r) = r else { bail!("the file list could not be fetched") };
        let path = dir.join(format!("{}.torrent", r.info_hash.as_string()));
        std::fs::write(&path, &r.torrent_bytes)?;
        parse_torrent(&r.torrent_bytes).context("the file list that came is not a torrent")?;
        Ok(path)
    }

    /// People are connected but nobody sends anything: drop them and ask the
    /// trackers and the DHT again (a stop and a start do both), so other
    /// people, who have the pieces, get a turn.
    pub async fn look_for_others(&self, gid: &str) -> anyhow::Result<()> {
        let h = self.handle(gid).await?;
        let session = self.session().await?.clone();
        self.heads.lock_ok().remove(gid);
        session.pause(&h).await?;
        session.unpause(&h).await?;
        Ok(())
    }

    /// Stop it now: what is here stays, and the notes of which pieces are
    /// here, so it goes on from there next time.
    pub async fn remove(&self, gid: &str) {
        self.heads.lock_ok().remove(gid);
        self.seen.lock_ok().remove(gid);
        if let (Some(session), Ok(id)) = (self.session.get(), Self::id(gid)) {
            if let Some(h) = session.get(id) {
                if !h.is_paused() {
                    let _ = session.pause(&h).await;
                }
            }
        }
    }

    /// Forget it altogether (its files stay where they are).
    pub async fn forget(&self, gid: &str) {
        self.heads.lock_ok().remove(gid);
        self.seen.lock_ok().remove(gid);
        self.sharing.lock_ok().remove(gid);
        if let (Some(session), Ok(id)) = (self.session.get(), Self::id(gid)) {
            let _ = session.delete(id, false).await;
        }
    }

    /// Complete: it shares with others until `minutes` passed or it sent
    /// `ratio` times its size, whichever comes first (0 = that one is no
    /// limit; both 0: no sharing), then it is forgotten and `leftovers`
    /// (bits of files that were not chosen) are deleted, with their folder
    /// when it is left empty.
    pub fn share_then_forget(self: &Arc<Self>, gid: &str, minutes: u32, ratio: f32, leftovers: Vec<PathBuf>) {
        self.heads.lock_ok().remove(gid);
        let turn = self.turns.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.sharing.lock_ok().insert(gid.to_string(), turn);
        let me = self.clone();
        let gid = gid.to_string();
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                let ours = me.sharing.lock_ok().get(&gid) == Some(&turn);
                if !ours {
                    return; // started again since: not ours to end
                }
                let done = match me.handle(&gid).await {
                    Ok(h) => {
                        let s = h.stats();
                        let sent = s.uploaded_bytes as f64 / s.total_bytes.max(1) as f64;
                        (minutes == 0 && ratio <= 0.0)
                            || (minutes > 0 && started.elapsed() >= Duration::from_secs(minutes as u64 * 60))
                            || (ratio > 0.0 && sent >= ratio as f64)
                            || matches!(s.state, TorrentStatsState::Paused | TorrentStatsState::Error)
                    }
                    Err(_) => true,
                };
                if done {
                    if me.sharing.lock_ok().get(&gid) == Some(&turn) {
                        me.forget(&gid).await;
                        for f in &leftovers {
                            if std::fs::remove_file(f).is_ok() {
                                if let Some(dir) = f.parent() {
                                    let _ = std::fs::remove_dir(dir); // only when empty
                                }
                            }
                        }
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }
}
