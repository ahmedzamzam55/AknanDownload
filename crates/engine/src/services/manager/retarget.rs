use super::Manager;
use crate::sync::{LockOk, RwOk};
use crate::filename;
use crate::model::{DownloadId, DownloadKind, Status};
use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

impl Manager {
    pub fn retarget(
        self: &Arc<Self>,
        id: DownloadId,
        dir: &Path,
        name: &str,
        description: Option<String>,
        auth: Option<(String, String)>,
    ) -> anyhow::Result<()> {
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut first_dir = None;
        let mut first_stem = String::new();
        let (done, running) = {
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
                    std::fs::create_dir_all(dir)?;
                    // it may be downloading already (in the folder first
                    // chosen): its temporary files go with it to the new
                    // folder (they stayed behind: the first folder's disk
                    // had to have room for all of it, and the file was
                    // copied over at the end), unless the user has a
                    // temporary folder of their own
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
            inner.dirty = true;
            (done, inner.entries.get(&id).map(|e| e.task.is_some()).unwrap_or(false))
        };
        // finished already (small file): move the file itself
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
        // the files are closed first: it stops for a moment
        if running {
            if let Some(t) = self.inner.lock_ok().entries.get(&id).and_then(|e| e.task.clone()) {
                t.cancel();
            }
            for _ in 0..250 {
                if self.inner.lock_ok().entries.get(&id).map(|e| e.task.is_none()).unwrap_or(true) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let mut stayed = false;
        for (from, to) in &moves {
            if std::fs::rename(from, to).is_ok() {
                continue;
            }
            // another disk: a small start is copied, a big one stays where it
            // is (copied once, when it is finished)
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
            // everything back where it was, the temporary files stay there
            for (from, to) in &moves {
                if to.exists() && !from.exists() {
                    let _ = std::fs::rename(to, from);
                }
            }
            // the same files as before: their folder and their name
            let back = first_dir.clone();
            self.update(id, move |x| {
                x.temp_dir = back;
                x.work_stem = Some(first_stem);
            });
        }
        if running {
            self.start(id);
        }
        Ok(())
    }
}
