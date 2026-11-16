use super::{disk_paths, Handle, Torrents};
use crate::sync::LockOk;
use crate::torrent::{FileState, Peer, TorrentStatus, PUBLIC_TRACKERS};
use librqbit::api::TorrentIdOrHash;
use librqbit::TorrentStatsState;
use std::collections::HashMap;
use std::time::Instant;

impl Torrents {
    pub async fn status(&self, gid: &str) -> anyhow::Result<TorrentStatus> {
        let h = self.handle(gid).await?;
        let s = h.stats();
        let status = match s.state {
            TorrentStatsState::Initializing { .. } => "waiting",
            TorrentStatsState::Live => "active",
            TorrentStatsState::Paused => "paused",
            TorrentStatsState::Error => "error",
        };
        let (speed, upload_speed, connections) = s
            .live
            .as_ref()
            .map(|l| (l.download_speed.as_bytes(), l.upload_speed.as_bytes(), l.snapshot.peer_stats.live))
            .unwrap_or_default();
        let (pieces, bitfield) = self.haves(&h);
        let mut trackers: Vec<String> = h.shared().trackers.iter().map(|u| u.to_string()).collect();
        for t in PUBLIC_TRACKERS {
            if !trackers.iter().any(|x| x == t) {
                trackers.push((*t).to_string());
            }
        }
        Ok(TorrentStatus {
            status: status.to_string(),
            total: s.total_bytes,
            done: s.progress_bytes,
            speed,
            upload_speed,
            connections,
            seeders: self.seen.lock_ok().get(gid).map(|p| p.sending).unwrap_or(0),
            seeding: s.finished,
            name: h.name().filter(|n| !n.is_empty()),
            followed_by: None,
            error: s.error.clone(),
            pieces,
            bitfield,
            trackers,
            upload_total: s.uploaded_bytes,
        })
    }

    /// How many pieces, and which are here (hex, a bit a piece).
    fn haves(&self, h: &Handle) -> (u32, String) {
        let Some(session) = self.session.get() else { return (0, String::new()) };
        let api = librqbit::Api::new(session.clone(), None);
        match api.api_dump_haves(TorrentIdOrHash::Hash(h.info_hash())) {
            Ok((bf, n)) => (n, bf.as_raw_slice().iter().map(|b| format!("{b:02x}")).collect()),
            Err(_) => (0, String::new()),
        }
    }

    /// The people this torrent is connected to (speeds: since the last look).
    pub async fn peers(&self, gid: &str) -> anyhow::Result<Vec<Peer>> {
        let h = self.handle(gid).await?;
        let Some(live) = h.live() else { return Ok(Vec::new()) };
        let snap = live.per_peer_stats_snapshot(Default::default());
        let mut seen = self.seen.lock_ok();
        let p = seen.entry(gid.to_string()).or_default();
        let secs = p.at.map(|a| a.elapsed().as_secs_f64()).filter(|s| *s > 0.2);
        let mut out = Vec::new();
        let mut now = HashMap::new();
        let mut sending = 0;
        for (addr, st) in snap.peers.iter().take(300) {
            let (got, gave) = (st.counters.fetched_bytes, st.counters.uploaded_bytes);
            let (got0, gave0) = p.bytes.get(addr).copied().unwrap_or((got, gave));
            let per = |a: u64, b: u64| secs.map(|s| (a.saturating_sub(b) as f64 / s) as u64).unwrap_or(0);
            let down = per(got, got0);
            if down > 0 {
                sending += 1;
            }
            out.push(Peer { address: addr.clone(), down, up: per(gave, gave0), have: -1.0, seeder: false });
            now.insert(addr.clone(), (got, gave));
        }
        p.bytes = now;
        p.at = Some(Instant::now());
        p.sending = sending;
        out.sort_by(|a, b| b.down.cmp(&a.down).then(b.up.cmp(&a.up)));
        Ok(out)
    }

    /// The files of a torrent once they are known, with how much of each is here.
    pub async fn files(&self, gid: &str) -> anyhow::Result<Vec<FileState>> {
        let h = self.handle(gid).await?;
        let s = h.stats();
        let only = h.only_files();
        let root = h.output_folder().to_path_buf();
        Ok(h.with_metadata(|m| {
            disk_paths(m)
                .into_iter()
                .zip(m.file_infos.iter())
                .enumerate()
                .filter_map(|(i, (p, f))| {
                    Some(FileState {
                        index: i + 1,
                        path: root.join(p?),
                        size: f.len,
                        done: s.file_progress.get(i).copied().unwrap_or(0).min(f.len),
                        selected: only.as_ref().is_none_or(|o| o.contains(&i)),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
    }
}

