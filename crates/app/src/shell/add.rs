use super::add_confirm::confirm_add;
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
                let url = d.get_url().trim().to_string();
                // another address: the download started for the old one stops
                if url != ctx.add_state.lock_ok().origin_url {
                    cancel_early(&ctx);
                }
                if swiftget_engine::torrent::is_magnet(&url) {
                    // typed / pasted a magnet link: that is the torrent window's job
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
                .lock()
                .unwrap()
                .categories
                .get(i.max(0) as usize)
                .cloned()
                .unwrap_or_else(|| "General".into());
            let current = PathBuf::from(d.get_save_as().as_str());
            let name = current
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "download".into());
            let s = ctx.mgr.settings();
            let path = s.dir_for(&cat).join(&name).display().to_string();
            d.set_save_as(path.clone().into());
            d.set_category_name(t(&cat).into());
            d.set_remember_path(s.category_dirs.contains_key(&cat));
            ctx.add_state.lock_ok().auto_path = path;
        });
    }
    {
        let w = add.as_weak();
        add.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            let current = PathBuf::from(d.get_save_as().as_str());
            let mut dlg = winfx::file_box();
            if let Some(dir) = current.parent().filter(|p| p.exists()) {
                dlg = dlg.set_directory(dir);
            }
            if let Some(name) = current.file_name() {
                dlg = dlg.set_file_name(name.to_string_lossy());
            }
            // Windows adds the file's own extension when a name is typed without it
            if let Some(ext) = current.extension().map(|e| e.to_string_lossy().into_owned()).filter(|e| !e.is_empty()) {
                dlg = dlg.add_filter(ext.to_ascii_uppercase(), &[ext.as_str()]).add_filter(t("All files"), &["*"]);
            }
            if let Some(p) = dlg.save_file() {
                let suggested = current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let name = p.file_name().map(|n| with_extension_of(&n.to_string_lossy(), &suggested)).unwrap_or_default();
                let p = p.with_file_name(name);
                d.set_save_as(p.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        add.on_start_now(move || confirm_add(&ui, true));
    }
    {
        // the server refused us: give the address back to the browser
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
        add.on_later(move || confirm_add(&ui, false));
    }
    {
        let ctx = ui.ctx.clone();
        add.on_cancel(move || {
            ctx.add_state.lock_ok().probe_seq += 1; // ignore a check still running
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

/// "Cancel" in Download File Info: the download that started meanwhile goes
/// away completely, with its temporary files and the file itself.
pub(super) fn cancel_early(ctx: &Ctx) {
    if let Some(id) = ctx.add_state.lock_ok().early_id.take() {
        ctx.mgr.discard(id);
    }
}

/// The window is free again: show the next link that came in meanwhile.
pub(super) fn open_next(ctx: &Ctx) {
    let next = ctx.add_state.lock_ok().waiting.pop_front();
    if let Some(p) = next {
        let ctx = ctx.clone();
        Timer::single_shot(Duration::from_millis(300), move || {
            open_add_dialog(&ctx, p.url, p.headers, p.name_hint, p.extra);
        });
    }
}

/// The window was closed without downloading: the server's answer kept for
/// the download is let go (its connection is not held any longer).
fn forget_look(ctx: &Ctx) {
    let url = ctx.add_state.lock_ok().origin_url.clone();
    if !url.is_empty() {
        swiftget_engine::http::drop_kept(&url);
    }
}

/// The download started in the background for the open window, if any.
pub(crate) fn early_download(ctx: &Ctx) -> Option<u64> {
    ctx.add_state.lock_ok().early_id
}
