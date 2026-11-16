use super::add::open_next;
use super::add_open::{apply_duplicate, finish_add, show_progress_for_start, with_extension_of};
use super::clipboard_ui::is_download_address;
use super::complete::show_complete;
use super::modal::{ask_file_there, message, FileThere};
use super::rows::refresh;
use super::state::{PendingAdd, Ui, VideoExtra};
use crate::i18n::{t, tf};
use crate::{extra, sound};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use swiftget_engine::sync::LockOk;
use swiftget_engine::{filename, NewDownload, Status};

/// Add the download from the "Download File Info" window.
pub(super) fn confirm_add(ui: &Ui, start: bool) {
    let ctx = &ui.ctx;
    let Some(d) = ctx.add.upgrade() else { return };
    let url = d.get_url().trim().to_string();
    if !is_download_address(&url) || url::Url::parse(&url).is_err() {
        d.set_info(t("Please enter a valid http://, https:// or ftp:// address").into());
        d.set_info_error(true);
        return;
    }
    // the checksum the file must have: one that is none is said at once
    let checksum_text = d.get_checksum().trim().to_string();
    let checksum = swiftget_engine::checksum::Expected::parse(&checksum_text).map(|e| e.text());
    if !checksum_text.is_empty() && checksum.is_none() {
        d.set_info(t("This is not a checksum: paste its MD5, SHA-1, SHA-256, SHA-384 or SHA-512 value").into());
        d.set_info_error(true);
        return;
    }
    let save_as = PathBuf::from(d.get_save_as().trim());
    // the name as the program suggested it (its extension is the real one)
    let suggested = {
        let st = ctx.add_state.lock_ok();
        let early = st.early_id.and_then(|id| ctx.mgr.get(id)).map(|v| v.file_name);
        let auto = Path::new(&st.auto_path).file_name().map(|n| n.to_string_lossy().into_owned());
        early.or(auto).or_else(|| st.probe.as_ref().map(|p| p.file_name.clone())).unwrap_or_default()
    };
    let name = save_as
        .file_name()
        .map(|n| with_extension_of(&n.to_string_lossy(), &suggested))
        .filter(|n| !n.trim().is_empty());
    let dir = save_as.parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty());
    // a file of that name is in the folder already: the user says what to do
    // (before, a number was put to the name without a word)
    let name = match (&dir, name) {
        (Some(dir), Some(n)) if dir.join(&n).is_file() => match ask_file_there(&n) {
            FileThere::Replace => {
                if let Err(e) = std::fs::remove_file(dir.join(&n)) {
                    message(&tf("Could not replace the file: {}", &[&e.to_string()]));
                    return;
                }
                Some(n)
            }
            FileThere::Number => {
                let numbered = filename::unique_name(dir, &n);
                d.set_save_as(dir.join(&numbered).display().to_string().into());
                Some(numbered)
            }
            FileThere::Cancel => return,
        },
        (_, n) => n,
    };
    let key = ctx
        .add_state
        .lock()
        .unwrap()
        .categories
        .get(d.get_category_index().max(0) as usize)
        .cloned()
        .unwrap_or_else(|| "General".into());
    {
        // ticked: this folder is the category's from now on; unticked where
        // one was remembered: forgotten (the usual folder again)
        let mut s = ctx.mgr.settings();
        let changed = match (&dir, d.get_remember_path()) {
            (Some(dir), true) => s.category_dirs.insert(key.clone(), dir.clone()).as_ref() != Some(dir),
            (_, false) => s.category_dirs.remove(&key).is_some(),
            _ => false,
        };
        if changed {
            let _ = ctx.mgr.set_settings(s);
        }
    }
    let settings = ctx.mgr.settings();
    let req = {
        let mut st = ctx.add_state.lock_ok();
        st.probe_seq += 1;
        let probe = if st.probe_url == url { st.probe.clone() } else { None };
        // the video extras belong to the original link only
        let extra = if st.origin_url == url { st.extra.clone() } else { VideoExtra::default() };
        // "Download Later" puts it in the chosen queue
        let queue = if start {
            None
        } else {
            settings.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id)
        };
        let description = d.get_description().trim().to_string();
        let mut headers = st.headers.clone();
        headers.auth = (d.get_use_login() && !d.get_user().trim().is_empty())
            .then(|| (d.get_user().trim().to_string(), d.get_password().to_string()));
        NewDownload {
            url: url.clone(),
            file_name: name,
            save_dir: dir,
            headers,
            connections: Some(d.get_connections().max(1) as u32),
            probe,
            audio_url: extra.audio_url,
            source_page: extra.source_page,
            source_key: extra.source_key,
            queue,
            description: (!description.is_empty()).then_some(description),
            pick_quality: None,
            group: None,
            early: false,
            start,
            torrent_select: None,
            clip: None,
            checksum: checksum.clone(),
            ..Default::default()
        }
    };
    let _ = d.hide();
    open_next(ctx);
    let early = ctx.add_state.lock_ok().early_id.take();
    if let Some(id) = early {
        if ctx.mgr.get(id).map(|v| v.url == url || v.parent_page.as_deref() == Some(url.as_str())).unwrap_or(false)
            || ctx.add_state.lock_ok().origin_url == url
        {
            let dir = req.save_dir.clone().unwrap_or_else(|| settings.dir_for("General"));
            let name = req.file_name.clone().unwrap_or_default();
            if let Err(e) = ctx.mgr.retarget(id, &dir, &name, req.description.clone(), req.headers.auth.clone()) {
                message(&tf("Could not change where it is saved: {}", &[&format!("{e:#}")]));
            }
            if checksum.is_some() {
                ctx.mgr.set_checksum(id, checksum.clone());
            }
            if start {
                if ctx.mgr.get(id).map(|v| v.status == Status::Failed || v.status == Status::Paused).unwrap_or(false) {
                    ctx.mgr.start(id);
                }
                // finished while the window was open: "Download complete"
                // now; still running: its progress window
                if ctx.mgr.get(id).map(|v| v.status == Status::Completed).unwrap_or(false) {
                    if settings.sounds_on {
                        sound::play(&settings.sound_complete);
                    }
                    if settings.show_complete_dialog {
                        show_complete(ui, id);
                    }
                } else {
                    show_progress_for_start(ui, id);
                }
            } else {
                // Download Later: stop it and put it in the queue
                ctx.mgr.pause(id);
                if let Some(q) = req.queue {
                    ctx.mgr.set_queue(id, Some(q));
                }
            }
            refresh(ui);
            return;
        }
        ctx.mgr.remove(id, false);
    }
    match ctx.mgr.find_url(&url) {
        Some(existing) => {
            let choice = settings.duplicate_action;
            if choice == 0 {
                extra::ask_duplicate(ui, PendingAdd { req, existing });
            } else {
                apply_duplicate(ui, PendingAdd { req, existing }, choice);
            }
        }
        None => {
            finish_add(ui, req);
        }
    }
}
