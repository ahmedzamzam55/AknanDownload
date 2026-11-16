use crate::torrent::{is_magnet, parse_torrent};
use librqbit::AddTorrent;
use librqbit_core::hash_id::Id20;
use std::str::FromStr;

/// What a running torrent can be changed to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Live {
    /// files to get, 1-based ("1,3-5"); None = all
    pub select: Option<String>,
    /// trackers the user added
    pub trackers: Vec<String>,
    /// the first and last pieces of the biggest file first
    pub first_last: bool,
}

/// "1,3-5" (1-based, as the window gives it) -> 0-based file numbers.
pub fn parse_select(sel: &str, count: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for part in sel.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().ok(), b.trim().parse::<usize>().ok()),
            None => {
                let n = part.parse::<usize>().ok();
                (n, n)
            }
        };
        if let (Some(a), Some(b)) = (a, b) {
            for i in a.max(1)..=b.min(count) {
                if !out.contains(&(i - 1)) {
                    out.push(i - 1);
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// The chosen files of a torrent given as its bytes.
pub(super) fn h_only(select: &Option<String>, add: &AddTorrent<'_>) -> Option<Vec<usize>> {
    let AddTorrent::TorrentFileBytes(b) = add else { return None };
    let n = parse_torrent(b).ok()?.files.len();
    select.as_deref().filter(|s| !s.trim().is_empty()).map(|s| parse_select(s, n)).filter(|v| !v.is_empty())
}

/// The torrent's id (its info hash, 40 hex) from a magnet link or a .torrent file.
pub fn hash_of(source: &str) -> Option<String> {
    if is_magnet(source) {
        let i = source.to_ascii_lowercase().find("xt=urn:btih:")? + "xt=urn:btih:".len();
        let h: String = source[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        // 40 hex, or 32 base32 (either case)
        return Id20::from_str(&h).or_else(|_| Id20::from_str(&h.to_ascii_uppercase())).ok().map(|id| id.as_string());
    }
    let data = std::fs::read(source).ok()?;
    parse_torrent(&data).ok()?.info_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_choices_and_ids() {
        assert_eq!(parse_select("1,3-5", 10), vec![0, 2, 3, 4]);
        assert_eq!(parse_select("4-2, 9, 12, x, 0", 10), vec![8]);
        assert_eq!(parse_select("2,2,1", 3), vec![0, 1]);
        let m = "magnet:?xt=urn:btih:0123456789ABCDEF0123456789abcdef01234567&dn=x";
        assert_eq!(hash_of(m).as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(hash_of("magnet:?xt=urn:btih:zz"), None);
        // the same id written in base32
        let b32 = "magnet:?xt=urn:btih:aerukz4jvpg66ajdivtytk6n54asgrlh&dn=x";
        assert_eq!(hash_of(b32).as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
    }
}
