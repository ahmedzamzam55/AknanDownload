use super::add_confirm::{confirm_add, ConflictMode};
use super::add_open::{open_add_dialog, with_extension_of};
use super::add_probe::start_probe;
use super::state::{Ctx, Ui};
use crate::i18n::t;
use crate::{api, torrent_ui, winfx};
use slint::{ComponentHandle, Timer};
use std::path::PathBuf;
use std::time::Duration;
use swiftget_engine::sync::LockOk;

pub(super) fn wire_add(ui: &Ui) {
    let Some(add) = ui.ctx.add.upgrade() else { return };
    {
        let ctx = ui.ctx.clone();
        add.on_check_url(move || {
            if let Some(d) = ctx.add.upgrade() {
                d.set_conflict_open(false);
                super::add_video::reset_media_panel(&ctx, &d);
                let url = d.get_url().trim().to_string();
                if url != ctx.add_state.lock_ok().origin_url {
                    cancel_early(&ctx);
                }
                if swiftget_engine::torrent::is_magnet(&url) {
                    let _ = d.hide();
                    torrent_ui::open_source(&url);
                    return;
                }
                if !url.is_empty() {
                    start_probe(&ctx, url);
                }
            }
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.on_category_changed(move |i| {
            let Some(d) = ctx.add.upgrade() else { return };
            let cat = ctx
                .add_state
                .lock_ok()
                .categories
                .get(i.max(0) as usize)
                .cloned()
                .unwrap_or_else(|| "General".into());
            let s = ctx.mgr.settings();
            let folder = s.dir_for(&cat).display().to_string();
            d.set_save_folder(folder.clone().into());
            d.set_category_name(t(&cat).into());
            d.set_remember_path(s.category_dirs.contains_key(&cat));
            d.set_conflict_open(false);
            ctx.add_state.lock_ok().auto_folder = folder;
        });
    }
    {
        let w = add.as_weak();
        add.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            let dir = PathBuf::from(d.get_save_folder().trim());
            let current_name = d.get_file_name().trim().to_string();
            let mut dlg = winfx::file_box();
            if dir.exists() {
                dlg = dlg.set_directory(&dir);
            }
            if !current_name.is_empty() {
                dlg = dlg.set_file_name(&current_name);
            }
            let ext = std::path::Path::new(&current_name)
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .filter(|e| !e.is_empty());
            if let Some(ext) = ext {
                dlg = dlg.add_filter(ext.to_ascii_uppercase(), &[ext.as_str()]).add_filter(t("All files"), &["*"]);
            }
            if let Some(p) = dlg.save_file() {
                if let Some(parent) = p.parent().filter(|x| !x.as_os_str().is_empty()) {
                    d.set_save_folder(parent.display().to_string().into());
                }
                if let Some(n) = p.file_name() {
                    let final_name = with_extension_of(&n.to_string_lossy(), &current_name);
                    d.set_file_name(final_name.into());
                }
                d.set_conflict_open(false);
            }
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.on_name_edited(move || {
            if let Some(d) = ctx.add.upgrade() {
                if d.get_conflict_open() {
                    d.set_conflict_open(false);
                }
            }
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.on_media_changed(move || {
            if let Some(d) = ctx.add.upgrade() {
                super::add_video::apply_selection_to_ui(&ctx, &d);
            }
        });
    }
    {
        let ui = ui.clone();
        add.on_conflict_replace(move || {
            let start = ui.ctx.add_state.lock_ok().conflict_start;
            confirm_add(&ui, start, ConflictMode::ReplaceSafely);
        });
    }
    {
        let ui = ui.clone();
        add.on_conflict_numbered(move || {
            let start = ui.ctx.add_state.lock_ok().conflict_start;
            confirm_add(&ui, start, ConflictMode::KeepNumbered);
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.on_conflict_rename(move || {
            if let Some(d) = ctx.add.upgrade() {
                d.set_conflict_open(false);
                d.invoke_focus_name();
            }
        });
    }
    {
        let ui = ui.clone();
        add.on_start_now(move || confirm_add(&ui, true, ConflictMode::Ask));
    }
    {
        let ctx = ui.ctx.clone();
        add.on_use_browser(move || {
            let Some(d) = ctx.add.upgrade() else { return };
            let url = d.get_url().trim().to_string();
            ctx.add_state.lock_ok().probe_seq += 1;
            cancel_early(&ctx);
            let _ = d.hide();
            if url.starts_with("http://") || url.starts_with("https://") {
                api::leave_to_browser(&url);
                let _ = open::that_detached(&url);
            }
            open_next(&ctx);
        });
    }
    {
        let ui = ui.clone();
        add.on_later(move || confirm_add(&ui, false, ConflictMode::Ask));
    }
    {
        let ctx = ui.ctx.clone();
        add.on_cancel(move || {
            ctx.add_state.lock_ok().probe_seq += 1;
            forget_look(&ctx);
            cancel_early(&ctx);
            if let Some(d) = ctx.add.upgrade() {
                let _ = d.hide();
            }
            open_next(&ctx);
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.window().on_close_requested(move || {
            ctx.add_state.lock_ok().probe_seq += 1;
            forget_look(&ctx);
            cancel_early(&ctx);
            open_next(&ctx);
            slint::CloseRequestResponse::HideWindow
        });
    }
}

/// "Cancel" in Download File Info: the pre-download in `.sgpart` is deleted cleanly.
pub(super) fn cancel_early(ctx: &Ctx) {
    if let Some(id) = ctx.add_state.lock_ok().early_id.take() {
        ctx.mgr.discard(id);
    }
}

pub(super) fn open_next(ctx: &Ctx) {
    let next = ctx.add_state.lock_ok().waiting.pop_front();
    if let Some(p) = next {
        let ctx = ctx.clone();
        Timer::single_shot(Duration::from_millis(300), move || {
            open_add_dialog(&ctx, p.url, p.headers, p.name_hint, p.extra);
        });
    }
}

fn forget_look(ctx: &Ctx) {
    let url = ctx.add_state.lock_ok().origin_url.clone();
    if !url.is_empty() {
        swiftget_engine::http::drop_kept(&url);
    }
}

pub(crate) fn early_download(ctx: &Ctx) -> Option<u64> {
    ctx.add_state.lock_ok().early_id
}
