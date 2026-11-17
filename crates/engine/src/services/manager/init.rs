use super::{Entry, Inner, limit_for, Manager, SpeedMeter};
use crate::sync::{LockOk, RwOk};
use crate::config::{AfterDone, Settings};
use crate::http::{build_client_with, Progress, ProxyConfig, Shared};
use crate::http_download;
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadKind, ProbeResult, Status};
use crate::ytdlp;
use crate::bt;
use crate::segments;
use crate::store;
use crate::system;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

/// Default place for settings.json and downloads.json
/// (%APPDATA%\Aknan\AknanDownload\data on Windows). The list and settings of
/// the earlier "SwiftGet" name are carried over the first time.
pub fn default_data_dir() -> PathBuf {
    let dir = directories::ProjectDirs::from("com", "Aknan", "AknanDownload")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("aknan-data"));
    if !dir.exists() {
        if let Some(old) = directories::ProjectDirs::from("com", "SwiftGet", "SwiftGet").map(|p| p.data_dir().to_path_buf()) {
            if old.exists() {
                let _ = std::fs::create_dir_all(&dir);
                for f in ["settings.json", "downloads.json"] {
                    let _ = std::fs::copy(old.join(f), dir.join(f));
                }
            }
        }
    }
    dir
}

/// The new link of a download whose old link stopped working gives the same
/// file: as big as before, a web page it is not, and it takes part requests.
pub(super) fn same_file(p: &ProbeResult, size: Option<u64>) -> anyhow::Result<()> {
    if p.is_web_page() || !p.resumable || size.is_none() || p.total_size != size {
        anyhow::bail!(
            "the new link is not the same file ({} instead of {}) - nothing was written; use \"Refresh download address\"",
            p.total_size.map(|n| n.to_string()).unwrap_or_else(|| "unknown size".into()),
            size.map(|n| n.to_string()).unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(())
}

impl Manager {
    pub fn new(rt: Handle, data_dir: PathBuf) -> anyhow::Result<Arc<Manager>> {
        std::fs::create_dir_all(&data_dir)?;
        let settings = Settings::load(&data_dir.join("settings.json"));
        ytdlp::set_mp3_kbps(settings.mp3_kbps);
        // sites that allowed only a few connections ("too many requests")
        if let Ok(text) = std::fs::read_to_string(data_dir.join("server-limits.json")) {
            // {"site": [connections, when]}
            if let Ok(map) = serde_json::from_str(&text) {
                http_download::set_learned(map);
            }
        }
        settings.create_folders();
        let proxy = ProxyConfig::from_settings(&settings);
        // a broken manual proxy must not keep the app from starting
        let client = build_client_with(&proxy).or_else(|_| build_client_with(&ProxyConfig { mode: 1, ..Default::default() }))?;
        let shared = Arc::new(Shared::new(client, SpeedLimiter::new(settings.effective_limit())));

        let mut entries = BTreeMap::new();
        let mut next_id = 1;
        for mut rec in store::load_downloads(&data_dir.join("downloads.json")) {
            if !matches!(rec.status, Status::Completed | Status::Failed | Status::Paused) {
                rec.status = Status::Paused;
            }
            let progress = Arc::new(Progress::default());
            let bytes = if rec.kind != DownloadKind::Hls && !rec.segments.is_empty() {
                segments::downloaded(&rec.segments) + segments::downloaded(&rec.audio_segments)
            } else {
                rec.downloaded
            };
            if rec.kind == DownloadKind::Hls {
                rec.resumable = true;
                if bytes > 0 && rec.parts_done > 0 && rec.parts_total >= rec.parts_done {
                    let est = ((bytes as u128 * rec.parts_total as u128) / rec.parts_done as u128) as u64;
                    progress.est_total.store(est.max(bytes), Ordering::Relaxed);
                    progress.hls_workers[0].0.store(bytes, Ordering::Relaxed);
                }
            }
            progress.bytes.store(bytes, Ordering::Relaxed);
            progress.parts_total.store(rec.parts_total, Ordering::Relaxed);
            progress.parts_done.store(rec.parts_done, Ordering::Relaxed);
            next_id = next_id.max(rec.id + 1);
            entries.insert(
                rec.id,
                Entry {
                    segs: Arc::new(Mutex::new(rec.segments.clone())),
                    audio_segs: Arc::new(Mutex::new(rec.audio_segments.clone())),
                    limiter: Arc::new(SpeedLimiter::new(rec.speed_limit_kb.unwrap_or(0) * 1024)),
                    rec,
                    progress,
                    task: None,
                    speed: SpeedMeter::default(),
                    quota_prev: 0,
                },
            );
        }
        let startup_queues: Vec<u64> =
            settings.queues.iter().filter(|q| q.schedule.on_startup).map(|q| q.id).collect();

        let mgr = Arc::new(Manager {
            inner: Mutex::new(Inner {
                entries,
                next_id,
                dirty: false,
                last_save: Instant::now(),
                sched_minute: None,
                completed: Vec::new(),
                exit_requested: false,
                when_all_done: AfterDone::Nothing,
                all_done_seen_active: false,
                events: Vec::new(),
                power_request: None,
                quota_log: VecDeque::new(),
                quota_paused: Vec::new(),
                quota_blocked: false,
                retry_waiting: false,
                queue_armed: HashSet::new(),
            }),
            settings: RwLock::new(settings),
            shared,
            proxy: Mutex::new(proxy),
            rt: rt.clone(),
            torrents: Arc::new(bt::Torrents::new(&data_dir.join("torrents"))),
            data_dir,
            relink: Mutex::new(HashSet::new()),
            waker: Mutex::new(None),
            torrent_live: Mutex::new(HashMap::new()),
            limit_now: std::sync::atomic::AtomicU64::new(u64::MAX),
            ticks: std::sync::atomic::AtomicU64::new(0),
        });

        for q in startup_queues {
            mgr.start_queue(q);
        }
        let m = mgr.clone();
        rt.spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(200));
            loop {
                every.tick().await;
                m.tick();
            }
        });
        Ok(mgr)
    }

    // ---------------------------------------------------------------- settings

    pub fn settings(&self) -> Settings {
        self.settings.read_ok().clone()
    }

    pub fn set_settings(self: &Arc<Self>, s: Settings) -> anyhow::Result<()> {
        let before = self.settings.read_ok().connections();
        if s.connections() != before {
            // Options > connections: every download that is not finished takes
            // the new number, a running one at once (no restart)
            let mut inner = self.inner.lock_ok();
            for e in inner.entries.values_mut() {
                if e.rec.status == Status::Completed || e.rec.kind == DownloadKind::Torrent {
                    continue;
                }
                let n = s.connections_for(&e.rec.url, s.connections());
                e.rec.connections = n;
                e.progress.want_connections.store(n, Ordering::Relaxed);
                // the user's own number: not raised again by itself
                e.progress.lowered_at.store(0, Ordering::Relaxed);
            }
            inner.dirty = true;
        }
        let limit = limit_for(&s, system::idle_secs());
        self.limit_now.store(limit, Ordering::Relaxed);
        self.shared.limiter.set_rate(limit);
        ytdlp::set_mp3_kbps(s.mp3_kbps);
        let proxy = ProxyConfig::from_settings(&s);
        if *self.proxy.lock_ok() != proxy {
            self.shared.set_client(build_client_with(&proxy)?);
            *self.proxy.lock_ok() = proxy;
        }
        s.save(&self.data_dir.join("settings.json"))?;
        s.create_folders();
        *self.settings.write_ok() = s;
        self.pump();
        Ok(())
    }

    pub fn runtime(&self) -> &Handle {
        &self.rt
    }
}
