use super::actions::{download_checked, explore};
use super::list::show;
use crate::*;
use std::sync::atomic::Ordering;
use swiftget_engine::grabber;

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    {
        let w = d.as_weak();
        d.on_template_changed(move |t| {
            if let Some(d) = w.upgrade() {
                if t != 6 {
                    d.set_exts(grabber::template_exts(t).join(" ").into());
                }
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_explore(move || explore(&ui));
    }
    {
        let ui = ui.clone();
        d.on_stop_explore(move || {
            if let Some(c) = ui.grab_state.borrow().cancel.clone() {
                c.cancel();
            }
            ui.grab_state.borrow().finished.store(true, Ordering::Relaxed);
        });
    }
    {
        let ui = ui.clone();
        d.on_toggle(move |i| {
            {
                let mut g = ui.grab_state.borrow_mut();
                if let Some(&idx) = g.shown.get(i.max(0) as usize) {
                    g.all[idx].checked = !g.all[idx].checked;
                }
            }
            show(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_check_all(move |on| {
            {
                let mut g = ui.grab_state.borrow_mut();
                let shown = g.shown.clone();
                for idx in shown {
                    g.all[idx].checked = on;
                }
            }
            show(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_filter_changed(move || show(&ui));
    }
    {
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = crate::winfx::file_box().set_directory(mgr.settings().download_dir).pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_download(move || download_checked(&ui));
    }
    {
        let ui = ui.clone();
        d.on_close_window(move || {
            if let Some(c) = ui.grab_state.borrow_mut().cancel.take() {
                c.cancel();
            }
            ui.grab_state.borrow_mut().exploring = false;
            if let Some(d) = ui.grabber.upgrade() {
                let _ = d.hide();
            }
        });
    }
    {
        let ui = ui.clone();
        d.window().on_close_requested(move || {
            if let Some(c) = ui.grab_state.borrow_mut().cancel.take() {
                c.cancel();
            }
            ui.grab_state.borrow_mut().exploring = false;
            slint::CloseRequestResponse::HideWindow
        });
    }
}
