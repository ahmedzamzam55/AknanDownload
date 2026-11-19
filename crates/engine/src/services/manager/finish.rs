use super::atomic_swap;
use super::Manager;
use crate::sync::LockOk;
use crate::filename;
use crate::http::Progress;
use crate::http_download::Cancelled;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, Step};
use crate::system;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Copy `from` to `to` (another disk), telling how far it is; stops at once
/// when `cancel` fires.
pub(super) fn copy_watched(from: &Path, to: &Path, on: &dyn Fn(f64), cancel: &CancellationToken) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let total = std::fs::metadata(from)?.len().max(1);
    let mut src = std::fs::File::open(from)?;
    let mut dst = std::io::BufWriter::with_capacity(4 << 20, std::fs::File::create(to)?);
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        done += n as u64;
        on(done as f64 / total as f64);
    }
    dst.flush()?;
    Ok(())
}

/// The finished file put in its place under `name` in `dir`, or a free
/// numbered name: a file that appeared there meanwhile is never replaced.
/// An antivirus or indexer that holds the new file for a moment does not
/// fail the download: it is tried again for a few seconds. (where, moved?)
/// Not moved: another disk (it is copied then).
pub(super) fn move_into_place(part: &Path, dir: &Path, name: &str) -> (PathBuf, bool) {
    let mut wait = 100;
    for _ in 0..12 {
        let target = if dir.join(name).exists() { dir.join(filename::unique_name(dir, name)) } else { dir.join(name) };
        match std::fs::rename(part, &target) {
            Ok(()) => return (target, true),
            // held by another program for a moment (access denied, in use)
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33)) && part.exists() => {
                std::thread::sleep(Duration::from_millis(wait));
                wait = (wait * 2).min(1000);
            }
            Err(_) => return (target, false),
        }
    }
    let target = if dir.join(name).exists() { dir.join(filename::unique_name(dir, name)) } else { dir.join(name) };
    (target, false)
}

/// A video or song file that is not one: a small file whose start is a web
/// page or a stream's list of pieces. The reason, for the user.
pub(super) fn not_the_media(part: &Path, file_name: &str) -> Option<String> {
    let media = matches!(
        filename::extension(file_name).as_str(),
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "m4v" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "ts"
    );
    let len = std::fs::metadata(part).ok()?.len();
    if !media || len == 0 || len > 512 * 1024 {
        return None;
    }
    let mut head = vec![0u8; 512.min(len as usize)];
    use std::io::Read;
    std::fs::File::open(part).ok()?.read_exact(&mut head).ok()?;
    let text = String::from_utf8_lossy(&head).trim_start_matches('\u{feff}').trim_start().to_ascii_lowercase();
    if text.starts_with("#extm3u") {
        return Some("the site gave a list of pieces of a video stream, not the video (it cannot be saved as one file this way): play the video on its page and use the add-in's button there".into());
    }
    if text.starts_with("<!doctype") || text.starts_with("<html") || text.starts_with("<?xml") || text.starts_with("{\"") {
        return Some("the site gave a web page, not the video (the site protects its videos, or the link has expired): play the video on its page and use the add-in's button there".into());
    }
    None
}

impl Manager {
    /// `.sgpart` -> real name (pick a new name if that one got taken meanwhile).
    /// To a folder on another disk it is copied, with its own bar.
    pub(super) async fn finish_file(&self, id: DownloadId, rec: &DownloadRecord, progress: &Arc<Progress>, cancel: &CancellationToken) -> anyhow::Result<()> {
        // name and folder may have changed while it was downloading
        let (save_dir, file_name) = {
            let inner = self.inner.lock_ok();
            inner
                .entries
                .get(&id)
                .map(|e| (e.rec.save_dir.clone(), e.rec.file_name.clone()))
                .unwrap_or_else(|| (rec.save_dir.clone(), rec.file_name.clone()))
        };
        let part = rec.part_path();
        if !part.exists() {
            progress.set_step(None);
            anyhow::bail!(
                "the downloaded data is gone: its temporary file \"{}\" was removed while it downloaded (an antivirus or a cleaning program?)",
                part.display()
            );
        }
        // a "video" of a few bytes: the site gave a page or a list instead
        // of the video (sites that protect their videos); never shown as done
        if let Some(why) = not_the_media(&part, &file_name) {
            progress.set_step(None);
            anyhow::bail!("{why}");
        }
        // the checksum it must have (the user's, or the server's)
        let want = {
            let inner = self.inner.lock_ok();
            inner.entries.get(&id).and_then(|e| e.rec.checksum.clone()).or_else(|| rec.checksum.clone())
        };
        if let Some(want) = want.as_deref().filter(|_| rec.kind == DownloadKind::Http && rec.clip.is_none()).and_then(crate::checksum::Expected::parse) {
            progress.set_step(Some(Step::Check));
            let (p2, pr, c2, w2) = (part.clone(), progress.clone(), cancel.clone(), want.clone());
            let same = tokio::task::spawn_blocking(move || crate::checksum::matches(&p2, &w2, &|f| pr.step_at(f), &|| c2.is_cancelled())).await?;
            progress.set_step(None);
            if cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            if !same? {
                anyhow::bail!(
                    "the downloaded file is damaged: its checksum ({}) is not the one it should have - download it again",
                    want.algo.name()
                );
            }
        }
        let _ = std::fs::create_dir_all(&save_dir);
        // every byte on the disk before the file is called done (Windows may
        // still hold some in memory: a shutdown right after would lose them)
        let p2 = part.clone();
        tokio::task::spawn_blocking(move || std::fs::OpenOptions::new().write(true).open(&p2).and_then(|f| f.sync_all())).await?.ok();

        // Option B safe hold: keep finished bytes inside `.sgpart` until "Start Download" is pressed
        let release = self.inner.lock_ok().entries.get(&id).map(|e| e.release.clone());
        let (mut save_dir, mut file_name, mut replace_existing) = (save_dir, file_name, rec.replace_existing);
        loop {
            let (hold, sd, fnm, rep) = {
                let inner = self.inner.lock_ok();
                inner
                    .entries
                    .get(&id)
                    .map(|e| (e.rec.hold_before_finish, e.rec.save_dir.clone(), e.rec.file_name.clone(), e.rec.replace_existing))
                    .unwrap_or((false, save_dir.clone(), file_name.clone(), replace_existing))
            };
            save_dir = sd;
            file_name = fnm;
            replace_existing = rep;
            if !hold {
                break;
            }
            let Some(notify) = &release else { break };
            progress.set_step(None);
            tokio::select! {
                _ = notify.notified() => {}
                _ = cancel.cancelled() => return Err(Cancelled.into()),
            }
        }
        let _ = std::fs::create_dir_all(&save_dir);
        let (p2, d2, n2) = (part.clone(), save_dir.clone(), file_name.clone());
        let (target, moved) = tokio::task::spawn_blocking(move || {
            if replace_existing {
                atomic_swap::replace_into_place(&p2, &d2, &n2)
            } else {
                move_into_place(&p2, &d2, &n2)
            }
        })
        .await?;
        if let Some(name) = target.file_name().and_then(|n| n.to_str()).filter(|n| *n != file_name) {
            let name = name.to_string();
            self.update(id, move |x| x.file_name = name);
        }
        if !moved {
            // temporary folder on another disk: copied safely via sibling temp file, then swapped
            progress.set_step(Some(Step::Move));
            let (from, to, p2, c2) = (part.clone(), target.clone(), progress.clone(), cancel.clone());
            tokio::task::spawn_blocking(move || {
                atomic_swap::cross_drive_swap(&from, &to, id, &|f| p2.step_at(f), &c2)
            })
            .await??;
        }
        progress.set_step(None);
        let settings = self.settings();
        if settings.set_file_date {
            if let Some(t) = rec.server_time {
                system::set_file_time(&target, t);
            }
        }
        if settings.virus_scan {
            system::virus_scan(&settings.virus_program, &settings.virus_args, &target);
        }
        Ok(())
    }

}
