use super::Manager;
use crate::filename;
use crate::model::{DownloadId, DownloadKind, Status};
use crate::sync::{LockOk, RwOk};
use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn finish_moves(
    mgr: &Arc<Manager>,
    id: DownloadId,
    moves: &[(PathBuf, PathBuf)],
    first_dir: Option<PathBuf>,
    first_stem: String,
    running: bool,
) {
    let mut stayed = false;
    for (from, to) in moves {
        if std::fs::rename(from, to).is_ok() {
            continue;
        }
        let small = from.is_file() && std::fs::metadata(from).map(|m| m.len() <= 512 << 20).unwrap_or(false);
        if small && std::fs::copy(from, to).is_ok() {
            let _ = std::fs::remove_file(from);
        } else {
            let _ = std::fs::remove_file(to);
            stayed = true;
            break;
        }
    }
    if stayed {
        for (from, to) in moves {
            if to.exists() && !from.exists() {
                let _ = std::fs::rename(to, from);
            }
        }
        mgr.update(id, move |x| {
            x.temp_dir = first_dir;
            x.work_stem = Some(first_stem);
        });
    }
    if running {
        mgr.start(id);
    }
}

impl Manager {
    /// Release a safe-held early pre-download (`Option B`) after "Start Download".
    #[allow(clippy::too_many_arguments)]
    pub fn confirm_early(
        self: &Arc<Self>,
        id: DownloadId,
        dir: &Path,
        name: &str,
        description: Option<String>,
        auth: Option<(String, String)>,
        checksum: Option<String>,
        replace_existing: bool,
    ) -> anyhow::Result<(bool, bool)> {
        let name = filename::sanitize(name);
            let (release, running, needs_start, completed) = {
                let mut inner = self.inner.lock_ok();
                let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
                e.rec.description = description.filter(|d| !d.trim().is_empty());
                if auth.is_some() || e.rec.headers.auth.is_some() {
                    e.rec.headers.auth = auth.filter(|(u, _)| !u.trim().is_empty());
                }
                if checksum.is_some() {
                    e.rec.checksum = checksum;
                }
                e.rec.replace_existing = replace_existing;
                e.rec.hold_before_finish = false;
                if e.rec.status != Status::Completed {
                    e.rec.save_dir = dir.to_path_buf();
                    e.rec.file_name = name;
                    e.rec.category = self.settings.read_ok().category_of(&e.rec.file_name, &e.rec.url);
                    e.rec.queue = None;
                }
                let running = e.task.is_some();
                let needs_start = !running && matches!(e.rec.status, Status::Paused | Status::Failed);
                let completed = e.rec.status == Status::Completed;
                let release = e.release.clone();
                inner.dirty = true;
                (release, running, needs_start, completed)
            };
        release.notify_one();
        if needs_start {
            self.start(id);
        }
        Ok((running || needs_start, completed))
    }

    /// Pause a safe-held early pre-download when the user clicks "Download Later",
    /// keeping partial `.sgpart` bytes out of `save_dir` until started later.
    #[allow(clippy::too_many_arguments)]
    pub fn pause_early(
        self: &Arc<Self>,
        id: DownloadId,
        dir: &Path,
        name: &str,
        description: Option<String>,
        auth: Option<(String, String)>,
        queue: Option<u64>,
        checksum: Option<String>,
        replace_existing: bool,
    ) -> anyhow::Result<()> {
        let name = filename::sanitize(name);
        {
            let mut inner = self.inner.lock_ok();
            let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
            e.rec.description = description.filter(|d| !d.trim().is_empty());
            if auth.is_some() || e.rec.headers.auth.is_some() {
                e.rec.headers.auth = auth.filter(|(u, _)| !u.trim().is_empty());
            }
            if checksum.is_some() {
                e.rec.checksum = checksum;
            }
            e.rec.save_dir = dir.to_path_buf();
            e.rec.file_name = name;
            e.rec.category = self.settings.read_ok().category_of(&e.rec.file_name, &e.rec.url);
            e.rec.replace_existing = replace_existing;
            e.rec.hold_before_finish = false;
            if let Some(q) = queue {
                e.rec.queue = Some(q);
            }
            if e.rec.status != Status::Completed {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
            inner.dirty = true;
        }
        Ok(())
    }

    pub fn retarget(
        self: &Arc<Self>,
        id: DownloadId,
        dir: &Path,
        name: &str,
        description: Option<String>,
        auth: Option<(String, String)>,
    ) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut first_dir = None;
        let mut first_stem = String::new();
        let (done, running, cancel_tok) = {
            let mut inner = self.inner.lock_ok();
            let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
            e.rec.description = description.filter(|d| !d.trim().is_empty());
            if auth.is_some() || e.rec.headers.auth.is_some() {
                e.rec.headers.auth = auth.filter(|(u, _)| !u.trim().is_empty());
            }
            let done = e.rec.status == Status::Completed;
            if !done {
                let name = filename::sanitize(name);
                if dir != e.rec.save_dir || name != e.rec.file_name {
                    let before = e.rec.temp_paths();
                    let own_temp = e.rec.temp_dir.as_ref().is_some_and(|t| !t.as_os_str().is_empty() && *t != e.rec.save_dir);
                    first_dir = Some(e.rec.work_dir());
                    first_stem = e.rec.temp_stem();
                    e.rec.save_dir = dir.to_path_buf();
                    e.rec.file_name = name;
                    e.rec.category = self.settings.read_ok().category_of(&e.rec.file_name, &e.rec.url);
                    if e.rec.work_stem.is_some() && !own_temp {
                        e.rec.temp_dir = Some(dir.to_path_buf());
                    }
                    let after = e.rec.temp_paths();
                    if e.rec.kind != DownloadKind::Torrent {
                        moves = before.into_iter().zip(after).filter(|(a, b)| a != b && a.exists()).collect();
                    }
                }
            }
            let tok = if !done && !moves.is_empty() { e.task.clone() } else { None };
            if tok.is_some() {
                e.rec.status = Status::Queued;
            }
            inner.dirty = true;
            (done, tok.is_some(), tok)
        };
        if done {
            let target = dir.join(filename::sanitize(name));
            let current = self.get(id).map(|v| v.save_path).unwrap_or_default();
            if target != current {
                self.move_file(id, &target, false)?;
            }
            return Ok(());
        }
        if moves.is_empty() {
            return Ok(());
        }
        if let Some(tok) = cancel_tok {
            tok.cancel();
            let me = self.clone();
            self.rt.spawn_blocking(move || {
                for _ in 0..250 {
                    if me.inner.lock_ok().entries.get(&id).map(|e| e.task.is_none()).unwrap_or(true) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                finish_moves(&me, id, &moves, first_dir, first_stem, running);
            });
            return Ok(());
        }
        finish_moves(self, id, &moves, first_dir, first_stem, false);
        Ok(())
    }
}
