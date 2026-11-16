use super::bencode::{info_span, Ben, Reader};
use anyhow::{anyhow, bail};
use std::path::{Path, PathBuf};

/// One file inside a torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFile {
    /// path inside the torrent's folder, with "/" between the parts
    pub path: String,
    pub size: u64,
    /// its number in the torrent, 1-based, counting the padding files the
    /// list leaves out (what "select-file" wants)
    pub index: usize,
}

/// What a .torrent file shares.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentInfo {
    /// the folder (several files) or file name
    pub name: String,
    pub files: Vec<TorrentFile>,
    pub total: u64,
    /// several files (in a folder named `name`), or one
    pub multi: bool,
    /// the trackers it names
    pub trackers: Vec<String>,
    /// its id (the SHA-1 of its "info" part, as a magnet link names it)
    pub info_hash: Option<String>,
}

/// More files than any real torrent has: refused (a file made to hurt).
const MAX_FILES: usize = 200_000;

/// A part of a path in a torrent as Windows can keep it safely: no "..",
/// no name of a device (CON, NUL...), none of \ / : * ? " < > |, no dot or
/// space at the end.
pub fn safe_part(part: &str) -> String {
    let p = part.trim();
    if p.is_empty() || p == "." || p == ".." {
        return "_".into();
    }
    let s = crate::filename::sanitize(p);
    if s.is_empty() || s.chars().all(|c| c == '.') {
        "_".into()
    } else {
        s
    }
}

/// The files whose path in the torrent is not safe to write as it is: their
/// number and the path to write them at instead (from the save folder, as
/// aria2's "index-out" wants it). Empty: every path is fine.
pub fn safe_paths(info: &TorrentInfo) -> Vec<(usize, String)> {
    let name = safe_part(&info.name);
    let name_changed = name != info.name;
    let mut out = Vec::new();
    for f in &info.files {
        let parts: Vec<&str> = f.path.split('/').collect();
        let safe: Vec<String> = parts.iter().map(|p| safe_part(p)).collect();
        let changed = name_changed || parts.iter().zip(&safe).any(|(a, b)| *a != b.as_str());
        if !changed {
            continue;
        }
        let rel = if info.multi { format!("{name}/{}", safe.join("/")) } else { name.clone() };
        out.push((f.index, rel));
    }
    out
}

/// A path written by the torrent program stays inside `dir`.
pub fn stays_inside(path: &Path, dir: &Path) -> bool {
    use std::path::Component;
    // "dir/./name" as aria2 writes it; a path given from the folder
    let clean = |p: &Path| p.components().filter(|c| !matches!(c, Component::CurDir)).collect::<PathBuf>();
    let (path, dir) = (clean(path), clean(dir));
    let rel = if path.is_absolute() || path.has_root() {
        match path.strip_prefix(&dir) {
            Ok(r) => r.to_path_buf(),
            Err(_) => return false,
        }
    } else {
        path
    };
    rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// The magnet link of a .torrent file (its id, name and trackers).
pub fn magnet_of(data: &[u8]) -> Option<String> {
    let info = parse_torrent(data).ok()?;
    let hash = info.info_hash?;
    let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
    let mut m = format!("magnet:?xt=urn:btih:{hash}&dn={}", enc(&info.name));
    for t in info.trackers.iter().take(20) {
        m.push_str("&tr=");
        m.push_str(&enc(t));
    }
    Some(m)
}

/// Read a .torrent file.
pub fn parse_torrent(data: &[u8]) -> anyhow::Result<TorrentInfo> {
    // a description, never more (the biggest real ones are a few MB)
    if data.len() > 32 << 20 {
        bail!("not a torrent file");
    }
    let root = Reader { data, pos: 0, depth: 0 }.value()?;
    let info = root.get("info").ok_or_else(|| anyhow!("not a torrent file"))?;
    let name = info
        .get("name.utf-8")
        .or_else(|| info.get("name"))
        .and_then(Ben::text)
        .unwrap_or_else(|| "torrent".into());
    let mut files = Vec::new();
    let multi = info.get("files").is_some();
    if info.get("files").is_some_and(|l| l.list().len() > MAX_FILES) {
        bail!("this torrent has too many files");
    }
    if let Some(list) = info.get("files") {
        for (n, f) in list.list().iter().enumerate() {
            let parts = f.get("path.utf-8").or_else(|| f.get("path")).map(Ben::list).unwrap_or(&[]);
            let path: Vec<String> = parts.iter().filter_map(Ben::text).collect();
            // padding files (BEP 47) are not real files
            let pad = f.get("attr").and_then(Ben::text).map(|a| a.contains('p')).unwrap_or(false)
                || path.first().map(|p| p.starts_with(".pad")).unwrap_or(false);
            if pad {
                continue;
            }
            files.push(TorrentFile {
                path: path.join("/"),
                size: f.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64,
                index: n + 1,
            });
        }
    } else {
        files.push(TorrentFile { path: name.clone(), size: info.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64, index: 1 });
    }
    if files.is_empty() {
        bail!("this torrent has no files");
    }
    let total = files.iter().fold(0u64, |a, f| a.saturating_add(f.size));
    let mut trackers: Vec<String> = Vec::new();
    for t in root.get("announce").and_then(Ben::text).into_iter().chain(
        root.get("announce-list").map(Ben::list).unwrap_or(&[]).iter().flat_map(|tier| tier.list().iter().filter_map(Ben::text)),
    ) {
        let t = t.trim().to_string();
        if !t.is_empty() && t.len() < 1024 && !trackers.contains(&t) {
            trackers.push(t);
        }
    }
    let info_hash = info_span(data).map(|(a, b)| {
        let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data[a..b]);
        d.as_ref().iter().map(|x| format!("{x:02x}")).collect()
    });
    Ok(TorrentInfo { name, files, total, multi, trackers, info_hash })
}

/// A magnet link of a torrent ("magnet:?xt=urn:btih:...").
pub fn is_magnet(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    // a real one is a few hundred characters
    s.len() <= 8192 && !s.contains(char::is_whitespace) && s.starts_with("magnet:?") && (s.contains("xt=urn:btih:") || s.contains("xt=urn:btmh:"))
}

/// Is this download a torrent: a magnet link or a .torrent file on the disk.
pub fn is_torrent_source(s: &str) -> bool {
    is_magnet(s) || (!s.contains("://") && s.to_ascii_lowercase().ends_with(".torrent"))
}

/// The name a magnet link gives ("dn="), if any.
pub fn magnet_name(uri: &str) -> Option<String> {
    let query = uri.trim().split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "dn").then(|| percent_encoding::percent_decode_str(&v.replace('+', " ")).decode_utf8_lossy().trim().to_string())
    }).filter(|n| !n.is_empty())
}
