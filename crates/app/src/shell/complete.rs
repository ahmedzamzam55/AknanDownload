use super::actions::{open_download, open_with, reveal_in_folder};
use super::format::kind_for;
use super::state::Ui;
use crate::i18n::tf;
use crate::winfx;
use slint::{ComponentHandle, SharedString};
use std::path::Path;
use swiftget_engine::{filename, fmt_size, DownloadView, Status};

pub(super) fn wire_complete(ui: &Ui) {
    let Some(complete) = ui.complete.upgrade() else { return };
    {
        let ui = ui.clone();
        complete.on_open_file(move || {
            if let Some(dir) = ui.complete_folder.borrow().clone() {
                open_download(&dir);
            } else if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                open_download(&v.save_path);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_open_with(move || {
            if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                open_with(&v.save_path);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_open_folder(move || {
            if let Some(dir) = ui.complete_folder.borrow().clone() {
                open_download(&dir);
            } else if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                reveal_in_folder(&v);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_close_window(move || close_complete(&ui));
    }
    {
        let ui = ui.clone();
        complete.on_drag_out(move || {
            // the finished file pulled out of the window: Windows takes it from here
            let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) else { return };
            let Some(d) = ui.complete.upgrade() else { return };
            if !v.save_path.is_file() {
                return;
            }
            winfx::drag_file_out(d.window(), &v.save_path);
            // the mouse was let go outside the window: the icon must know too
            d.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: slint::LogicalPosition::new(0.0, 0.0),
                button: slint::platform::PointerEventButton::Left,
            });
            d.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
        });
    }
}

/// A playlist (or files added together) all done: one window for all.
pub(super) fn show_group_complete(ui: &Ui, group: u64, all: usize, done: usize, failed: usize) {
    let Some(d) = ui.complete.upgrade() else { return };
    let views: Vec<DownloadView> = ui.ctx.mgr.snapshot().into_iter().filter(|v| v.group == Some(group)).collect();
    let Some(first) = views.iter().find(|v| v.status == Status::Completed).or(views.first()).cloned() else { return };
    // the folder they went to (the one most of them share)
    let folder = first.save_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let bytes: u64 = views.iter().filter(|v| v.status == Status::Completed).filter_map(|v| v.total_size).sum();
    ui.complete_id.set(None);
    *ui.complete_folder.borrow_mut() = Some(folder.clone());
    let title = first.description.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| tf("{} files", &[&all]));
    d.set_name(title.into());
    d.set_url(if failed > 0 { tf("{} finished, {} could not be downloaded", &[&done, &failed]) } else { tf("All {} finished", &[&done]) }.into());
    d.set_size(fmt_size(bytes).into());
    d.set_path(folder.display().to_string().into());
    d.set_kind(kind_for(&first.file_name).into());
    d.set_ext(SharedString::new());
    d.set_dont_show(false);
    open_window!(d);
}

pub(crate) fn show_complete(ui: &Ui, id: u64) {
    if let (Some(v), Some(d)) = (ui.ctx.mgr.get(id), ui.complete.upgrade()) {
        *ui.complete_folder.borrow_mut() = None;
        ui.complete_id.set(Some(id));
        d.set_name(v.file_name.clone().into());
        d.set_url(v.url.clone().into());
        d.set_size(v.total_size.map(fmt_size).unwrap_or_default().into());
        d.set_path(v.save_path.display().to_string().into());
        d.set_kind(kind_for(&v.file_name).into());
        d.set_ext(filename::extension(&v.file_name).to_uppercase().into());
        d.set_dont_show(false);
        open_window!(d);
    }
}

fn close_complete(ui: &Ui) {
    if let Some(d) = ui.complete.upgrade() {
        if d.get_dont_show() {
            let mut s = ui.ctx.mgr.settings();
            s.show_complete_dialog = false;
            let _ = ui.ctx.mgr.set_settings(s);
        }
        let _ = d.hide();
    }
}
