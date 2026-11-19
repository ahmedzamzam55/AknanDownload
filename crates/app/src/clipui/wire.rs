use super::actions::{fetch_video, preview_part, queue_part};
use super::{show_length, STATE};
use crate::*;
use swiftget_engine::{clock, parse_clock};

pub(super) fn wire(ui: &Ui, d: &ClipDialog) {
    let w = d.as_weak();
    d.on_close_window(move || {
        if let Some(d) = w.upgrade() {
            let _ = d.hide();
        }
        STATE.with(|s| {
            if let Some(c) = s.borrow_mut().preview.take() {
                c.cancel();
            }
        });
    });
    d.window().on_close_requested(move || {
        STATE.with(|s| {
            if let Some(c) = s.borrow_mut().preview.take() {
                c.cancel();
            }
        });
        slint::CloseRequestResponse::HideWindow
    });
    let w = d.as_weak();
    d.on_fetch(move || {
        if let Some(d) = w.upgrade() {
            fetch_video(&d);
        }
    });
    // the sliders: the times follow; the end stays after the start
    let w = d.as_weak();
    d.on_slid(move || {
        let Some(d) = w.upgrade() else { return };
        let dur = d.get_duration();
        if d.get_to() <= d.get_from() {
            d.set_to((d.get_from() + 1.0).min(dur.max(d.get_from() + 1.0)));
        }
        d.set_from_text(clock(d.get_from() as f64, ':').into());
        d.set_to_text(clock(d.get_to() as f64, ':').into());
        d.set_message(SharedString::new());
        show_length(&d);
    });
    let w = d.as_weak();
    d.on_from_typed(move |text| {
        let Some(d) = w.upgrade() else { return };
        if let Some(v) = parse_clock(&text) {
            let dur = d.get_duration();
            d.set_from(if dur > 0.0 { (v as f32).min(dur) } else { v as f32 });
            d.set_message(SharedString::new());
            show_length(&d);
        }
    });
    let w = d.as_weak();
    d.on_to_typed(move |text| {
        let Some(d) = w.upgrade() else { return };
        if let Some(v) = parse_clock(&text) {
            let dur = d.get_duration();
            d.set_to(if dur > 0.0 { (v as f32).min(dur) } else { v as f32 });
            d.set_message(SharedString::new());
            show_length(&d);
        }
    });
    // a look at the part: the part alone, quick, in a small quality
    let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
    d.on_preview(move || {
        if let Some(d) = w.upgrade() {
            preview_part(&d, &mgr);
        }
    });
    let (w, u) = (d.as_weak(), ui.clone());
    d.on_download(move || {
        if let Some(d) = w.upgrade() {
            queue_part(&d, &u);
        }
    });
}
