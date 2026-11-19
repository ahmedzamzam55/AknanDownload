use super::add::open_next;
use super::add_open::{apply_duplicate, finish_add, show_progress_for_start, with_extension_of};
use super::clipboard_ui::is_download_address;
use super::complete::show_complete;
use super::modal::message;
use super::rows::refresh;
use super::state::{PendingAdd, Ui, VideoExtra};
use crate::i18n::{t, tf};
use crate::{extra, sound};
use slint::ComponentHandle;
use std::path::PathBuf;
use swiftget_engine::sync::LockOk;
use swiftget_engine::{filename, NewDownload};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConflictMode {
    Ask,
    ReplaceSafely,
    KeepNumbered,
}

/// Add the download from the "Download File Info" window.
pub(super) fn confirm_add(ui: &Ui, start: bool, mode: ConflictMode) {
    let ctx = &ui.ctx;
    let Some(d) = ctx.add.upgrade() else { return };
    let url = d.get_url().trim().to_string();
    if !is_download_address(&url) || url::Url::parse(&url).is_err() {
        d.set_info(t("Please enter a valid http://, https:// or ftp:// address").into());
        d.set_info_error(true);
        return;
    }
    let checksum_text = d.get_checksum().trim().to_string();
    let checksum = swiftget_engine::checksum::Expected::parse(&checksum_text).map(|e| e.text());
    if !checksum_text.is_empty() && checksum.is_none() {
        d.set_info(t("This is not a checksum: paste its MD5, SHA-1, SHA-256, SHA-384 or SHA-512 value").into());
        d.set_info_error(true);
        return;
    }
    let settings = ctx.mgr.settings();
    let suggested = {
        let st = ctx.add_state.lock_ok();
        let early = st.early_id.and_then(|id| ctx.mgr.get(id)).map(|v| v.file_name);
        early
            .or_else(|| (!st.auto_name.is_empty()).then(|| st.auto_name.clone()))
            .or_else(|| st.probe.as_ref().map(|p| p.file_name.clone()))
            .unwrap_or_else(|| "download".into())
    };
    let typed_name = d.get_file_name().trim().to_string();
    let raw_name = if typed_name.is_empty() { suggested.clone() } else { typed_name.clone() };
    let mut file_name = filename::sanitize(&with_extension_of(&raw_name, &suggested));
    if file_name.trim().is_empty() {
        file_name = "download".into();
    }
    let folder_raw = d.get_save_folder().trim().to_string();
    let dir = if folder_raw.is_empty() {
        settings.dir_for("General")
    } else {
        PathBuf::from(folder_raw)
    };

    // Inline conflict prompt inside AddDialog — old file is NEVER deleted early!
    let mut replace_existing = false;
    if dir.join(&file_name).is_file() {
        match mode {
            ConflictMode::Ask => {
                let numbered = filename::unique_name(&dir, &file_name);
                ctx.add_state.lock_ok().conflict_start = start;
                d.set_numbered_name(numbered.into());
                d.set_conflict_open(true);
                return;
            }
            ConflictMode::KeepNumbered => {
                file_name = filename::unique_name(&dir, &file_name);
                d.set_file_name(file_name.clone().into());
            }
            ConflictMode::ReplaceSafely => {
                replace_existing = true;
            }
        }
    }
    d.set_conflict_open(false);

    let key = ctx
        .add_state
        .lock_ok()
        .categories
        .get(d.get_category_index().max(0) as usize)
        .cloned()
        .unwrap_or_else(|| "General".into());
    {
        let mut s = ctx.mgr.settings();
        let changed = match d.get_remember_path() {
            true => s.category_dirs.insert(key.clone(), dir.clone()).as_ref() != Some(&dir),
            false => s.category_dirs.remove(&key).is_some(),
        };
        if changed {
            let _ = ctx.mgr.set_settings(s);
        }
    }
    let mut req = {
        let mut st = ctx.add_state.lock_ok();
        st.probe_seq += 1;
        let probe = if st.probe_url == url { st.probe.clone() } else { None };
        let extra = if st.origin_url == url { st.extra.clone() } else { VideoExtra::default() };
        let queue = if start { None } else { settings.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id) };
        let description = d.get_description().trim().to_string();
        let mut headers = st.headers.clone();
        headers.auth = (d.get_use_login() && !d.get_user().trim().is_empty())
            .then(|| (d.get_user().trim().to_string(), d.get_password().to_string()));
        NewDownload {
            url: url.clone(),
            file_name: Some(file_name.clone()),
            save_dir: Some(dir.clone()),
            headers,
            connections: Some(d.get_connections().max(1) as u32),
            probe,
            audio_url: extra.audio_url,
            source_page: extra.source_page,
            source_key: extra.source_key,
            queue,
            description: (!description.is_empty()).then_some(description),
            subtitle: extra.subtitle,
            sub_convert: extra.sub_convert,
            sub_body: extra.sub_body,
            checksum: checksum.clone(),
            replace_existing,
            start,
            ..Default::default()
        }
    };
    let media_applied = super::add_video::apply_media_choice(ctx, &d, &mut req, &typed_name);
    let file_name = req.file_name.clone().unwrap_or(file_name);
    let url = req.url.clone();
    let _ = d.hide();
    open_next(ctx);

    let early = ctx.add_state.lock_ok().early_id.take();
    if let Some(id) = early {
        let matches = !media_applied
            && (ctx.mgr.get(id).map(|v| v.url == url || v.parent_page.as_deref() == Some(url.as_str())).unwrap_or(false)
                || ctx.add_state.lock_ok().origin_url == url);
        if matches {
            let desc = req.description.clone();
            let auth = req.headers.auth.clone();
            if start {
                match ctx.mgr.confirm_early(id, &dir, &file_name, desc, auth, checksum, replace_existing) {
                    Ok((_, true)) => {
                        if settings.sounds_on {
                            sound::play(&settings.sound_complete);
                        }
                        if settings.show_complete_dialog {
                            show_complete(ui, id);
                        }
                    }
                    Ok(_) => show_progress_for_start(ui, id),
                    Err(e) => message(&tf("Could not change where it is saved: {}", &[&format!("{e:#}")])),
                }
            } else if let Err(e) = ctx.mgr.pause_early(id, &dir, &file_name, desc, auth, req.queue, checksum, replace_existing) {
                message(&tf("Could not change where it is saved: {}", &[&format!("{e:#}")]));
            }
            refresh(ui);
            return;
        }
        ctx.mgr.discard(id);
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
