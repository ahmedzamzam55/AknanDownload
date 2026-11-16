//! Torrents (.torrent files and magnet links).
//!
//! Reading .torrent files and magnet links, and what the windows show of a
//! running torrent. The sharing itself is in [`crate::bt`] (librqbit, inside
//! the program).
//!
//! By default nothing is shared (seeded) after a torrent is complete; the
//! user can choose minutes of sharing in Options > Torrents.


mod bencode;
mod parse;
mod status;
#[cfg(test)]
mod tests;

pub use parse::{
    is_magnet, is_torrent_source, magnet_name, magnet_of, parse_torrent, safe_part, safe_paths, stays_inside,
    TorrentFile, TorrentInfo,
};
pub use status::{is_tracker, piece_runs, share_of, AddOptions, FileState, Peer, TorrentStatus, PUBLIC_TRACKERS};
