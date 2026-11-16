use super::{Inner, Manager};
use crate::sync::LockOk;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Status};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `path` is in `dir` (or below it), with no ".." that could lead out.
pub(super) fn inside(path: &Path, dir: &Path) -> bool {
    path.starts_with(dir)
        && path != dir
        && !path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir))
}

/// The files of a torrent, then its folders if they are empty now.
pub(super) fn remove_torrent_data(rec: &DownloadRecord) -> bool {
    let mut ok = true;
    // only inside its own folder: a strange name can't reach anything else
    let files: Vec<PathBuf> = rec.torrent_data_paths().into_iter().filter(|f| inside(f, &rec.save_dir)).collect();
    for f in &files {
        if f.is_file() {
            ok &= std::fs::remove_file(f).is_ok();
        }
    }
    let top = rec.target_path();
    if !inside(&top, &rec.save_dir) {
        return ok;
    }
    let mut dirs: Vec<PathBuf> = files.iter().filter_map(|f| f.parent().map(Path::to_path_buf)).filter(|d| d.starts_with(&top)).collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for d in dirs.iter().chain(std::iter::once(&top)) {
        let _ = std::fs::remove_dir(d); // only when empty
    }
    ok
}

/// Delete every temporary file of a download. Returns false if something
/// could not be removed (still open), so the caller can retry.
/// Every file another download of the list uses (its temporary files and
/// its file), but those of `except`.
pub(super) fn paths_in_use_but(inner: &Inner, except: DownloadId) -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    for e in inner.entries.values().filter(|e| e.rec.id != except) {
        if e.rec.kind != DownloadKind::Torrent {
            set.extend(e.rec.temp_paths());
        }
        set.insert(e.rec.target_path());
    }
    set
}

impl Manager {
    /// Every file a download of the list uses now.
    pub(super) fn paths_in_use(&self) -> HashSet<PathBuf> {
        paths_in_use_but(&self.inner.lock_ok(), DownloadId::MAX)
    }
}

/// The temporary files of `rec` go, never one that a download still in
/// the list uses (a download removed while a new one of the same file
/// started took that one's file away: it went on into nothing).
pub(super) fn remove_temp_files(rec: &DownloadRecord, in_use: &HashSet<PathBuf>) -> bool {
    let mut ok = true;
    if rec.kind == DownloadKind::Torrent && rec.status != Status::Completed {
        // what an unfinished torrent got so far is its partial data
        ok &= remove_torrent_data(rec);
    }
    for p in rec.temp_paths().into_iter().filter(|p| !in_use.contains(p)) {
        if p.is_dir() {
            ok &= std::fs::remove_dir_all(&p).is_ok();
        } else if p.exists() {
            ok &= std::fs::remove_file(&p).is_ok();
        }
    }
    ok
}
