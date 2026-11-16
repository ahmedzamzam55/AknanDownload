use super::{disk_paths, h_only, hash_of, parse_select, Handle, Torrents};
use crate::sync::LockOk;
use crate::torrent::{is_magnet, is_tracker, parse_torrent, AddOptions};
use anyhow::{bail, Context};
use librqbit::limits::LimitsConfig;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, TorrentStatsState};
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::time::Duration;

impl Torrents {
    /// Start (or go on with) a torrent; gives its id.
    pub async fn add(&self, source: &str, o: &AddOptions) -> anyhow::Result<String> {
        let session = self.session().await?.clone();
        let source = source.trim();
        let mut count = None;
        let add = if is_magnet(source) {
            AddTorrent::from_url(source.to_string())
        } else {
            let data = std::fs::read(source).context("the .torrent file can't be read")?;
            count = Some(parse_torrent(&data)?.files.len());
            AddTorrent::from_bytes(data)
        };
        // the chosen files from the start (known for a .torrent file)
        let only = count.zip(o.select.as_deref().filter(|s| !s.trim().is_empty())).map(|(n, s)| parse_select(s, n)).filter(|v| !v.is_empty());
        // known already: as it was asked now
        if let Some(gid) = hash_of(source) {
            self.sharing.lock_ok().remove(&gid);
            if let Some(h) = session.get(Self::id(&gid)?) {
                let same = h.output_folder() == o.dir
                    && o.trackers.iter().all(|t| !is_tracker(t) || h.shared().trackers.iter().any(|u| u.as_str() == t.trim()))
                    && self.own_limit.lock_ok().get(&gid).copied().unwrap_or(0) == o.download_limit_kb;
                if o.check || self.files_gone(&h) {
                    // checked again from nothing
                    self.forget(&gid).await;
                } else if !same {
                    let bytes = h.with_metadata(|m| m.torrent_bytes.clone())?;
                    let h = self.start_again(&gid, AddTorrent::from_bytes(bytes), o).await?;
                    let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
                    return Ok(gid);
                } else {
                    let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
                    if h.is_paused() {
                        session.unpause(&h).await?;
                    }
                    return Ok(gid);
                }
            }
            self.folders.lock_ok().insert(gid.clone(), o.dir.clone());
        }
        let h = self.start(add, o, only).await?;
        let gid = h.info_hash().as_string();
        self.sharing.lock_ok().remove(&gid);
        // (a magnet link's files are chosen once it is ready: the next change)
        let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
        Ok(gid)
    }

    async fn start(&self, add: AddTorrent<'_>, o: &AddOptions, only: Option<Vec<usize>>) -> anyhow::Result<Handle> {
        let session = self.session().await?.clone();
        let limit = |kb: u64| NonZeroU32::new(kb.saturating_mul(1024).min(u32::MAX as u64) as u32);
        let opts = AddTorrentOptions {
            overwrite: true,
            output_folder: Some(o.dir.to_string_lossy().into_owned()),
            ratelimits: LimitsConfig { download_bps: limit(o.download_limit_kb), upload_bps: None },
            trackers: Some(o.trackers.iter().map(|t| t.trim().to_string()).filter(|t| is_tracker(t)).collect()),
            only_files: only,
            ..Default::default()
        };
        let h = match session.add_torrent(add, Some(opts)).await? {
            AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => bail!("the torrent was not started"),
        };
        if h.is_paused() {
            session.unpause(&h).await?;
        }
        let gid = h.info_hash().as_string();
        self.folders.lock_ok().insert(gid.clone(), o.dir.clone());
        self.own_limit.lock_ok().insert(gid, o.download_limit_kb);
        Ok(h)
    }

    /// Started again with other options (folder, trackers, its own limit):
    /// librqbit forgets a torrent with its notes of which pieces are here,
    /// so the notes are kept aside meanwhile and nothing is checked again.
    pub(super) async fn start_again(&self, gid: &str, add: AddTorrent<'_>, o: &AddOptions) -> anyhow::Result<Handle> {
        let session = self.session().await?.clone();
        let notes_dir = self.data_dir.join("session");
        let notes = std::fs::read_dir(&notes_dir)
            .ok()
            .and_then(|d| {
                d.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                    p.extension().is_some_and(|x| x == "bitv")
                        && p.file_stem().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(gid))
                })
            });
        let kept = notes.as_ref().and_then(|n| std::fs::read(n).ok());
        if let Some(h) = session.get(Self::id(gid)?) {
            let _ = session.pause(&h).await;
        }
        self.heads.lock_ok().remove(gid);
        session.delete(Self::id(gid)?, false).await.ok();
        if let (Some(n), Some(bytes)) = (&notes, &kept) {
            let _ = std::fs::write(n, bytes);
        }
        self.folders.lock_ok().insert(gid.to_string(), o.dir.clone());
        let only = h_only(&o.select, &add);
        self.start(add, o, only).await
    }

    /// Files it had on the disk are gone (deleted while it was stopped):
    /// checked again from nothing, not taken as there.
    fn files_gone(&self, h: &Handle) -> bool {
        let only = h.only_files();
        let st = h.stats();
        h.with_metadata(|m| {
            disk_paths(m).iter().enumerate().any(|(i, p)| {
                let wanted = only.as_ref().is_none_or(|o| o.contains(&i));
                let had = st.file_progress.get(i).copied().unwrap_or(0) > 0;
                p.as_ref().is_some_and(|p| wanted && had && !h.output_folder().join(p).exists())
            })
        })
        .unwrap_or(false)
    }

    /// The files to get, and "first and last pieces first".
    pub(super) async fn apply(&self, gid: &str, h: &Handle, select: Option<&str>, first_last: bool) -> anyhow::Result<()> {
        if matches!(h.stats().state, TorrentStatsState::Initializing { .. }) {
            bail!("the torrent is not ready yet");
        }
        let count = h.with_metadata(|m| m.file_infos.len()).unwrap_or(0);
        if count > 0 {
            let want: HashSet<usize> = match select.filter(|s| !s.trim().is_empty()) {
                Some(s) => parse_select(s, count).into_iter().collect(),
                None => (0..count).collect(),
            };
            let now: HashSet<usize> = h.only_files().map(|v| v.into_iter().collect()).unwrap_or_else(|| (0..count).collect());
            if !want.is_empty() && want != now {
                self.session().await?.update_only_files(h, &want).await?;
            }
        }
        self.first_last(gid, h, first_last).await;
        Ok(())
    }

    /// Two streams on the biggest wanted file, one at its start and one at
    /// its end: librqbit asks for the pieces just ahead of each first.
    async fn first_last(&self, gid: &str, h: &Handle, on: bool) {
        self.heads.lock_ok().remove(gid);
        if !on || h.stats().finished {
            return;
        }
        let only = h.only_files();
        let biggest = h
            .with_metadata(|m| {
                m.file_infos
                    .iter()
                    .enumerate()
                    .filter(|(i, f)| !f.attrs.padding && only.as_ref().is_none_or(|o| o.contains(i)))
                    .max_by_key(|(_, f)| f.len)
                    .map(|(i, f)| (i, f.len))
            })
            .ok()
            .flatten();
        let Some((file, len)) = biggest else { return };
        let mut streams = Vec::new();
        for back in [None, Some(len.saturating_sub(2 << 20))] {
            let Ok(Ok(mut s)) = tokio::time::timeout(Duration::from_secs(5), h.clone().stream(file)).await else { break };
            if let Some(pos) = back {
                use tokio::io::AsyncSeekExt;
                if s.seek(std::io::SeekFrom::Start(pos)).await.is_err() {
                    continue;
                }
            }
            streams.push(Box::new(s) as Box<dyn std::any::Any + Send>);
        }
        if !streams.is_empty() {
            self.heads.lock_ok().insert(gid.to_string(), streams);
        }
    }
}

