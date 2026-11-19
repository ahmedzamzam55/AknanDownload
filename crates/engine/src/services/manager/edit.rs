use super::Manager;
use crate::sync::{LockOk, RwOk};
use crate::filename;
use crate::model::{DownloadId, RequestHeaders, Status};
use anyhow::anyhow;
use std::path::Path;
use std::sync::Arc;

/// Editable fields of the File Properties window.
#[derive(Debug, Clone, Default)]
pub struct DetailsEdit {
    pub url: Option<String>,
    pub referer: Option<String>,
    pub description: Option<String>,
    pub auth: Option<(String, String)>,
    /// the checksum the file must have ("": none; None: unchanged)
    pub checksum: Option<String>,
}

impl Manager {
    /// Move / rename a finished file ("Save as" for downloads that are done).
    /// `replace`: the user agreed to replace a file of that name there.
    pub fn move_file(&self, id: DownloadId, new_path: &Path, replace: bool) -> anyhow::Result<()> {
        // the list is not held while the file moves: a copy to another disk
        // can take minutes and everything else must go on meanwhile
        let from = {
            let inner = self.inner.lock_ok();
            let e = inner.entries.get(&id).ok_or_else(|| anyhow!("download not found"))?;
            if e.rec.status != Status::Completed {
                anyhow::bail!("only finished downloads can be moved");
            }
            e.rec.target_path()
        };
        if from == new_path {
            return Ok(());
        }
        // a file of that name there already is replaced only when the user said so
        if new_path.exists() && !replace {
            anyhow::bail!("a file with this name is already in that folder: {}", new_path.display());
        }
        if let Some(dir) = new_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if let Err(e) = std::fs::rename(&from, new_path) {
            if !from.exists() {
                return Err(e.into());
            }
            // another drive: a copy under a passing name first, then the real
            // name, and only then the old file goes
            let tmp = new_path.with_file_name(format!(
                ".{}.aknan-move",
                new_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ));
            let copied = std::fs::copy(&from, &tmp).and_then(|n| {
                let same = std::fs::metadata(&from).map(|m| m.len() == n).unwrap_or(false);
                if same {
                    Ok(())
                } else {
                    Err(std::io::Error::other("the copy is not complete"))
                }
            });
            if let Err(e) = copied.and_then(|_| {
                if new_path.exists() && !replace {
                    return Err(std::io::Error::other("a file with this name appeared in that folder"));
                }
                std::fs::rename(&tmp, new_path)
            }) {
                let _ = std::fs::remove_file(&tmp);
                return Err(anyhow!("the file could not be moved: {e}"));
            }
            let _ = std::fs::remove_file(&from);
        }
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.save_dir = new_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            if let Some(n) = new_path.file_name() {
                e.rec.file_name = n.to_string_lossy().into_owned();
            }
            e.rec.category = self.settings.read_ok().category_of(&e.rec.file_name, &e.rec.url);
        }
        inner.dirty = true;
        Ok(())
    }

    /// Put every download back into the category its file type says
    /// (after categories were added, changed or deleted).
    pub fn recategorize(&self) {
        let s = self.settings();
        let names = s.category_names();
        let mut inner = self.inner.lock_ok();
        for e in inner.entries.values_mut() {
            let wanted = s.category_of(&e.rec.file_name, &e.rec.url);
            if !names.contains(&e.rec.category) || wanted != "General" {
                e.rec.category = wanted;
            }
        }
        inner.dirty = true;
    }

    /// "Refresh download address": continue an unfinished download from a
    /// new link (the old one expired).
    pub fn set_url(self: &Arc<Self>, id: DownloadId, url: &str, headers: Option<RequestHeaders>) {
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            if let Some(t) = &e.task {
                t.cancel();
            }
            e.rec.url = url.to_string();
            e.rec.final_url = None;
            if let Some(h) = headers {
                let auth = e.rec.headers.auth.clone();
                e.rec.headers = h;
                if e.rec.headers.auth.is_none() {
                    e.rec.headers.auth = auth;
                }
            }
            if e.rec.status == Status::Failed {
                e.rec.status = Status::Paused;
                e.rec.error = None;
            }
        }
        inner.dirty = true;
    }

    /// Changes from the File Properties window.
    pub fn update_details(&self, id: DownloadId, d: DetailsEdit) -> anyhow::Result<()> {
        let mut inner = self.inner.lock_ok();
        let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
        if let Some(u) = d.url.filter(|u| !u.trim().is_empty() && *u != e.rec.url) {
            if e.rec.status == Status::Completed {
                anyhow::bail!("the address of a finished download can't be changed");
            }
            e.rec.url = u.trim().to_string();
            e.rec.final_url = None;
        }
        e.rec.headers.referer = d.referer.filter(|r| !r.trim().is_empty());
        e.rec.description = d.description.filter(|r| !r.trim().is_empty());
        if let Some(c) = d.checksum {
            e.rec.checksum = crate::checksum::Expected::parse(&c).map(|x| x.text());
        }
        e.rec.headers.auth = d.auth.filter(|(u, _)| !u.trim().is_empty());
        inner.dirty = true;
        Ok(())
    }

    /// Unfinished download: save to another folder / name (nothing written yet,
    /// or the temporary file moves along).
    pub fn change_target(&self, id: DownloadId, dir: &Path, name: &str) -> anyhow::Result<()> {
        let clean = filename::sanitize(name);
        let (old, new_temps) = {
            let inner = self.inner.lock_ok();
            let e = inner.entries.get(&id).ok_or_else(|| anyhow!("download not found"))?;
            if e.task.is_some() {
                anyhow::bail!("stop the download first");
            }
            if e.rec.status == Status::Completed {
                anyhow::bail!("use Move / Rename for finished files");
            }
            let mut preview = e.rec.clone();
            preview.save_dir = dir.to_path_buf();
            preview.file_name = clean.clone();
            (e.rec.clone(), preview.temp_paths())
        };
        std::fs::create_dir_all(dir)?;
        let pairs: Vec<_> = old
            .temp_paths()
            .into_iter()
            .zip(new_temps)
            .filter(|(from, to)| from.exists() && from != to)
            .collect();
        let mut moved = Vec::new();
        for (from, to) in pairs {
            if std::fs::rename(&from, &to).is_err() {
                for (a, b) in moved.into_iter().rev() {
                    let _ = std::fs::rename(b, a);
                }
                anyhow::bail!("could not move the partly downloaded data");
            }
            moved.push((from, to));
        }
        let mut inner = self.inner.lock_ok();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.save_dir = dir.to_path_buf();
            e.rec.file_name = clean;
            e.rec.auto_name = false;
            inner.dirty = true;
        }
        Ok(())
    }
}
