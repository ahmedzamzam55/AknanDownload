//! Torrents, shared by librqbit inside the program (no helper program).
//!
//! One session for the whole program, started when the first torrent needs
//! it. It remembers which pieces of each torrent are on the disk (a stopped
//! torrent goes on without checking every piece again, after a restart too)
//! and the people it found (the DHT list), in the program's own folder.
//!
//! Files are written by our own store ([`SafeFiles`]): every part of a name
//! made safe for Windows first, the same way the .torrent window shows it.

use crate::sync::LockOk;
use crate::torrent::PUBLIC_TRACKERS;
use anyhow::{anyhow, Context};
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    DhtSessionConfig, ListenerMode, ListenerOptions, ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
};
use librqbit_core::hash_id::Id20;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

type Handle = Arc<ManagedTorrent>;

mod lifecycle;
mod ops;
mod select;
mod status;
mod storage;

pub use select::{hash_of, parse_select, Live};
pub use storage::{disk_paths, SafeFiles, SafeStore};
use select::h_only;
use storage::Folders;

#[derive(Default)]
struct PeerSeen {
    at: Option<Instant>,
    /// address -> bytes from them, bytes to them
    bytes: HashMap<String, (u64, u64)>,
    /// how many sent something since the last look
    sending: u32,
}

/// The torrents of the program.
pub struct Torrents {
    data_dir: PathBuf,
    session: tokio::sync::OnceCell<Arc<Session>>,
    seen: Mutex<HashMap<String, PeerSeen>>,
    /// "first and last pieces first": the open streams that ask for them
    heads: Mutex<HashMap<String, Vec<Box<dyn std::any::Any + Send>>>>,
    folders: Folders,
    /// a download's own speed limit (KB/s) each torrent was started with
    own_limit: Mutex<HashMap<String, u64>>,
    /// torrents that share for a while after they are complete (a number,
    /// so a later start of the same torrent ends the old sharing)
    sharing: Mutex<HashMap<String, u64>>,
    turns: std::sync::atomic::AtomicU64,
}

impl Torrents {
    pub fn new(data_dir: &Path) -> Torrents {
        Torrents {
            data_dir: data_dir.to_path_buf(),
            session: tokio::sync::OnceCell::new(),
            seen: Mutex::default(),
            heads: Mutex::default(),
            folders: Folders::default(),
            own_limit: Mutex::default(),
            sharing: Mutex::default(),
            turns: Default::default(),
        }
    }

    async fn session(&self) -> anyhow::Result<&Arc<Session>> {
        self.session
            .get_or_try_init(|| async {
                std::fs::create_dir_all(&self.data_dir)?;
                // the folders of the torrents it kept from last time
                let saved = self.data_dir.join("session").join("session.json");
                if let Ok(v) = std::fs::read(&saved).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<serde_json::Value>(&b)?)) {
                    if let Some(all) = v["torrents"].as_object() {
                        let mut f = self.folders.lock_ok();
                        for t in all.values() {
                            if let (Some(h), Some(dir)) = (t["info_hash"].as_str(), t["output_folder"].as_str()) {
                                f.insert(h.to_ascii_lowercase(), PathBuf::from(dir));
                            }
                        }
                    }
                }
                // no IPv6 on this computer (turned off, or none): IPv4 only
                let v4 = std::net::TcpListener::bind((std::net::Ipv6Addr::UNSPECIFIED, 0)).is_err();
                let trackers: HashSet<url::Url> = PUBLIC_TRACKERS.iter().filter_map(|t| url::Url::parse(t).ok()).collect();
                let opts = SessionOptions {
                    dht: Some(DhtSessionConfig {
                        persistence: Some(librqbit::dht::DhtPersistenceConfig {
                            config_filename: Some(self.data_dir.join("dht.json")),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    fastresume: true,
                    persistence: Some(SessionPersistenceConfig::Json { folder: Some(self.data_dir.join("session")) }),
                    listen: Some(ListenerOptions {
                        mode: ListenerMode::TcpOnly,
                        enable_upnp_port_forwarding: true,
                        listen_addr: if v4 { (std::net::Ipv4Addr::UNSPECIFIED, 0).into() } else { (std::net::Ipv6Addr::UNSPECIFIED, 0).into() },
                        ipv4_only: v4,
                        ..Default::default()
                    }),
                    ipv4_only: v4,
                    default_storage_factory: Some(SafeFiles { folders: self.folders.clone() }.boxed()),
                    trackers,
                    peer_limit: Some(150),
                    // the streams of "first and last pieces first" each keep one
                    runtime_worker_threads: Some(16),
                    ..Default::default()
                };
                let session = Session::new_with_opts(self.data_dir.join("unused"), opts).await.context("torrents can't start")?;
                // what ran last time waits until its download is started again
                let running: Vec<Handle> =
                    session.with_torrents(|all| all.filter(|(_, t)| !t.is_paused()).map(|(_, t)| t.clone()).collect());
                for t in running {
                    let _ = session.pause(&t).await;
                }
                Ok::<_, anyhow::Error>(session)
            })
            .await
    }

    fn id(gid: &str) -> anyhow::Result<TorrentIdOrHash> {
        Ok(TorrentIdOrHash::Hash(Id20::from_str(gid).map_err(|_| anyhow!("not a torrent id"))?))
    }

    async fn handle(&self, gid: &str) -> anyhow::Result<Handle> {
        self.session().await?.get(Self::id(gid)?).ok_or_else(|| anyhow!("the torrent was stopped"))
    }

    /// The speed limits of all torrents together (bytes/s, 0 = none).
    pub async fn set_limits(&self, down: u64, up: u64) {
        let Some(session) = self.session.get() else { return };
        let bps = |b: u64| NonZeroU32::new(b.min(u32::MAX as u64) as u32);
        if session.ratelimits.get_download_bps() != bps(down) {
            session.ratelimits.set_download_bps(bps(down));
        }
        if session.ratelimits.get_upload_bps() != bps(up) {
            session.ratelimits.set_upload_bps(bps(up));
        }
    }
}

