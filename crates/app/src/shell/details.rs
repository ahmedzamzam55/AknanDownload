use super::details_fill::fill_details;
use super::details_torrent::wire_torrent_tab;
use super::format::{completion_from, completion_index};
use super::look::{set_theme_of, Look};
use super::state::{ProgressWin, Ui};
use super::window::saved_size;
use crate::i18n::t;
use crate::{tray, DetailsDialog, SegView, Theme};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{fmt_size, DownloadKind, DownloadView, Manager, Status};

pub(crate) fn open_progress(ui: &Ui, id: u64) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    if let Some((d, segs)) = ui.progress.borrow().get(&id) {
        // back from next to the clock
        tray::remove_download(id);
        fill_details(d, segs, &v);
        open_window!(d);
        d.window().set_minimized(false);
        return;
    }
    let Ok(d) = DetailsDialog::new() else { return };
    let settings = ui.ctx.mgr.settings();
    set_theme_of(d.global::<Theme>(), &Look::of(&settings));
    // the details (connections) only when the user chose to see them; the
    // window opens as wide / tall as it was last time
    d.set_show_details(settings.progress_details);
    {
        let saved = saved_size(&ui.ctx.mgr, "progress");
        // the 3D looks show the ring and the cards: a little wider
        let least = if settings.theme >= 9 { 600.0 } else { 440.0 };
        let width = saved.map(|s| s.0).unwrap_or(500.0).max(least);
        let small = d.get_content_height();
        let height = if settings.progress_details { saved.map(|s| s.1).unwrap_or(0.0).max(small) } else { small };
        d.window().set_size(slint::LogicalSize::new(width, height.max(120.0)));
    }
    // the tabs' controls are set once, so typing in them isn't overwritten
    d.set_limit_on(v.speed_limit_kb.is_some());
    d.set_limit_kb(v.speed_limit_kb.unwrap_or(500).min(1_000_000) as i32);
    d.set_when_done(completion_index(v.on_complete));
    d.set_complete_dialog(settings.show_complete_dialog);
    d.set_force_close(settings.force_shutdown);
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_limit_changed(move || {
            if let Some(d) = w.upgrade() {
                let kb = d.get_limit_kb().max(1) as u64;
                mgr.set_download_limit(id, d.get_limit_on().then_some(kb));
            }
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_completion_changed(move || {
            if let Some(d) = w.upgrade() {
                mgr.set_on_complete(id, completion_from(d.get_when_done()));
                let mut s = mgr.settings();
                if s.show_complete_dialog != d.get_complete_dialog() || s.force_shutdown != d.get_force_close() {
                    s.show_complete_dialog = d.get_complete_dialog();
                    s.force_shutdown = d.get_force_close();
                    let _ = mgr.set_settings(s);
                }
            }
        });
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_pause(move || mgr.pause(id));
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_resume(move || mgr.start(id));
    }
    {
        // "Cancel": stop the download and close the window
        let (mgr, map) = (ui.ctx.mgr.clone(), ui.progress.clone());
        d.on_cancel_download(move || {
            mgr.pause(id);
            close_progress(&map, id);
        });
    }
    {
        let map = ui.progress.clone();
        d.on_close_window(move || close_progress(&map, id));
    }
    {
        // "Hide next to the clock": the window goes, a small icon with the
        // progress comes next to the clock, the download goes on
        let (map, mgr, w) = (ui.progress.clone(), ui.ctx.mgr.clone(), d.as_weak());
        d.on_hide_to_tray(move || {
            let Some(d) = w.upgrade() else { return };
            let (tip, progress) = mgr.get(id).map(|v| tray_tip(&v)).unwrap_or_default();
            if tray::show_download(id, &tip, progress, (&t("Show download progress"), &t("Stop download"))) {
                let _ = d.hide();
            } else {
                // no icon could be made: at least out of the way
                d.window().set_minimized(true);
            }
            let _ = &map;
        });
    }
    {
        // "Hide details": the window gets small; "Show details": back to
        // the height it had
        // the choice is kept for the next windows
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_details_toggled(move || {
            let (w, mgr) = (w.clone(), mgr.clone());
            let mut st = mgr.settings();
            if let Some(d) = w.upgrade() {
                if st.progress_details != d.get_show_details() {
                    st.progress_details = d.get_show_details();
                    let _ = mgr.set_settings(st);
                }
            }
            slint::Timer::single_shot(Duration::from_millis(1), move || {
                let Some(d) = w.upgrade() else { return };
                let scale = d.window().scale_factor();
                let now = d.window().size().to_logical(scale);
                let height = if d.get_show_details() {
                    let before = saved_size(&mgr, "progress").map(|s| s.1).unwrap_or(0.0);
                    before.max(d.get_content_height())
                } else {
                    d.get_content_height()
                };
                d.window().set_size(slint::LogicalSize::new(now.width, height.max(120.0)));
            });
        });
    }
    {
        // Alt+F4: the same
        let (map, mgr) = (ui.progress.clone(), ui.ctx.mgr.clone());
        d.window().on_close_requested(move || {
            stop_and_close(&map, &mgr, id);
            slint::CloseRequestResponse::HideWindow
        });
    }
    let segs = Rc::new(VecModel::<SegView>::default());
    d.set_segs(ModelRc::from(segs.clone()));
    if v.kind == DownloadKind::Torrent {
        wire_torrent_tab(ui, &d, id, &v);
    }
    fill_details(&d, &segs, &v);
    open_window!(d);
    ui.progress.borrow_mut().insert(id, (d, segs));
}

/// The X of a download's window: the download stops (it can be resumed).
fn stop_and_close(map: &Rc<RefCell<HashMap<u64, ProgressWin>>>, mgr: &Arc<Manager>, id: u64) {
    if mgr.get(id).is_some_and(|v| v.status != Status::Completed) {
        mgr.pause(id);
    }
    close_progress(map, id);
}

/// The hint and the bar of a hidden download's icon next to the clock.
pub(super) fn tray_tip(v: &DownloadView) -> (String, Option<f32>) {
    let progress = v.total_size.filter(|t| *t > 0).map(|_| v.progress());
    let mut tip = match progress {
        Some(p) => format!("{:.0}%  {}", p * 100.0, v.file_name),
        None => format!("{}  {}", fmt_size(v.downloaded), v.file_name),
    };
    if v.waiting {
        tip.push_str(&format!("\n{}", t("Waiting for connection…")));
    } else if v.status.is_running() && v.speed > 0.0 {
        tip.push_str(&format!("\n{}/s", fmt_size(v.speed as u64)));
    } else if v.status == Status::Paused {
        tip.push_str(&format!("\n{}", t("Paused")));
    }
    (tip, progress)
}

pub(super) fn close_progress(map: &Rc<RefCell<HashMap<u64, ProgressWin>>>, id: u64) {
    tray::remove_download(id);
    if let Some((d, _)) = map.borrow().get(&id) {
        let _ = d.hide();
    }
    let map = map.clone();
    slint::Timer::single_shot(Duration::from_millis(1), move || {
        map.borrow_mut().remove(&id);
    });
}
