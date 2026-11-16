use std::path::PathBuf;

/// The state of one torrent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TorrentStatus {
    /// "active", "waiting", "paused", "error", "complete", "removed"
    pub status: String,
    pub total: u64,
    pub done: u64,
    pub speed: u64,
    pub upload_speed: u64,
    pub connections: u32,
    pub seeders: u32,
    /// all chosen files are here (it may still share them)
    pub seeding: bool,
    /// the torrent's name, once known (magnet links learn it first)
    pub name: Option<String>,
    /// a magnet link got its file list: the torrent itself goes on here
    pub followed_by: Option<String>,
    pub error: Option<String>,
    /// how many pieces, and which are here (hex, a bit a piece)
    pub pieces: u32,
    pub bitfield: String,
    /// the trackers it asks
    pub trackers: Vec<String>,
    /// bytes sent to others
    pub upload_total: u64,
}

/// One person a torrent is connected to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Peer {
    pub address: String,
    pub down: u64,
    pub up: u64,
    /// how much of the torrent they have (0..1; below 0: not known)
    pub have: f32,
    pub seeder: bool,
}

/// How much of the pieces a bitfield (hex) says are there (0..1).
pub fn share_of(bitfield: &str) -> f32 {
    if bitfield.is_empty() {
        return 0.0;
    }
    let ones: u32 = bitfield.chars().filter_map(|c| c.to_digit(16)).map(|d| d.count_ones()).sum();
    ones as f32 / (bitfield.len() as f32 * 4.0)
}

/// Which pieces are here, as up to `n` runs: (from, to, here) in 0..1.
pub fn piece_runs(bitfield: &str, pieces: u32, n: usize) -> Vec<(f32, f32, bool)> {
    if pieces == 0 || bitfield.is_empty() || n == 0 {
        return Vec::new();
    }
    let has = |i: u32| -> bool {
        let c = bitfield.as_bytes().get((i / 4) as usize).and_then(|c| (*c as char).to_digit(16)).unwrap_or(0);
        c & (8 >> (i % 4)) != 0
    };
    let cells = (pieces as usize).min(n);
    let mut out: Vec<(f32, f32, bool)> = Vec::new();
    for cell in 0..cells {
        let a = (cell as u64 * pieces as u64 / cells as u64) as u32;
        let b = (((cell as u64 + 1) * pieces as u64 / cells as u64) as u32).max(a + 1);
        // a cell is "here" when all its pieces are
        let here = (a..b).all(has);
        let (from, to) = (cell as f32 / cells as f32, (cell + 1) as f32 / cells as f32);
        match out.last_mut() {
            Some(last) if last.2 == here => last.1 = to,
            _ => out.push((from, to, here)),
        }
    }
    out
}

/// Public trackers added to every torrent (the torrent's own come first).
/// Long-running, open ones that answer most torrents.
pub const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://explodie.org:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.dler.org:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "https://tracker.tamersunion.org:443/announce",
    "http://tracker.opentrackr.org:1337/announce",
];

/// Options for one torrent.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub dir: PathBuf,
    /// files to get, 1-based ("1,3-5"); `None` = all
    pub select: Option<String>,
    /// the download's own limit, KB/s (0 = only the limit of all downloads)
    pub download_limit_kb: u64,
    /// check every piece on the disk again (not what was noted of them)
    pub check: bool,
    /// the first and last pieces of the biggest file first (a video plays sooner)
    pub first_last: bool,
    /// trackers the user added (besides the torrent's own and the public ones)
    pub trackers: Vec<String>,
}

/// An address a tracker can have (udp, http, https; no spaces or commas).
pub fn is_tracker(t: &str) -> bool {
    let l = t.trim().to_ascii_lowercase();
    (l.starts_with("udp://") || l.starts_with("http://") || l.starts_with("https://"))
        && t.len() < 1024
        && !t.contains([',', ' ', '\n', '\r'])
        && url::Url::parse(t.trim()).is_ok()
}

/// One file of a running torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    /// its number in the torrent (1-based)
    pub index: usize,
    pub path: PathBuf,
    pub size: u64,
    pub done: u64,
    /// one of the files the user wants (the others only get the bits of
    /// pieces they share with those)
    pub selected: bool,
}

