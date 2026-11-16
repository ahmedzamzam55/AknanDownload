use crate::i18n::{t, tf};
use crate::*;

pub(crate) fn ask_duplicate(ui: &Ui, p: PendingAdd) {
    let Some(d) = ui.dup.upgrade() else { return };
    let v = ui.ctx.mgr.get(p.existing);
    d.set_file_name(v.as_ref().map(|v| v.file_name.clone()).unwrap_or_default().into());
    d.set_existing_status(v.as_ref().map(|v| v.status.label()).unwrap_or("").into());
    d.set_choice(1);
    d.set_remember(false);
    *ui.pending.borrow_mut() = Some(p);
    open_window!(d);
}

const COUNTDOWN: i32 = 30;

pub(crate) fn start_countdown(ui: &Ui, action: AfterDone) {
    let Some(d) = ui.countdown.upgrade() else { return };
    let text = match action {
        AfterDone::Shutdown => "shut down the computer",
        AfterDone::Hibernate => "hibernate the computer",
        _ => "put the computer to sleep",
    };
    d.set_action_text(t(text).into());
    d.set_seconds_total(COUNTDOWN);
    d.set_seconds_left(COUNTDOWN);
    *ui.power.borrow_mut() = Some((action, COUNTDOWN));
    open_window!(d);
}

pub(crate) fn tick_countdown(ui: &Ui) {
    // a file still being put together (joined, checked, moved): the time
    // waits for it, then starts again from the beginning
    let waiting = if ui.power.borrow().is_some() { ui.ctx.mgr.finishing() } else { None };
    let left = {
        let mut p = ui.power.borrow_mut();
        let Some((_, left)) = p.as_mut() else { return };
        if waiting.is_some() {
            *left = COUNTDOWN;
        } else {
            *left -= 1;
        }
        *left
    };
    if let Some(d) = ui.countdown.upgrade() {
        d.set_seconds_left(left.max(0));
        d.set_waiting_text(match &waiting {
            Some(name) => tf("Waiting for \"{}\" to be finished first (its parts are being put together).", &[name]).into(),
            None => SharedString::new(),
        });
    }
    if left <= 0 {
        power_now(ui);
    }
}

fn power_now(ui: &Ui) {
    // ("Now" while a file is still being put together: it waits for it)
    if ui.ctx.mgr.finishing().is_some() {
        if let Some(p) = ui.power.borrow_mut().as_mut() {
            p.1 = 1;
        }
        return;
    }
    let Some((action, _)) = ui.power.borrow_mut().take() else { return };
    if let Some(d) = ui.countdown.upgrade() {
        let _ = d.hide();
    }
    // save the list first: the downloads resume after the restart
    ui.ctx.mgr.shutdown();
    let force = ui.ctx.mgr.settings().force_shutdown;
    swiftget_engine::system::power(action, force);
}

pub(super) fn wire(ui: &Ui) {
    if let Some(d) = ui.dup.upgrade() {
        {
            let ui = ui.clone();
            d.on_ok(move || {
                let Some(d) = ui.dup.upgrade() else { return };
                let choice = d.get_choice().clamp(1, 3) as u32;
                if d.get_remember() {
                    let mut s = ui.ctx.mgr.settings();
                    s.duplicate_action = choice;
                    let _ = ui.ctx.mgr.set_settings(s);
                }
                let _ = d.hide();
                if let Some(p) = ui.pending.borrow_mut().take() {
                    apply_duplicate(&ui, p, choice);
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_cancel(move || {
                ui.pending.borrow_mut().take();
                if let Some(d) = ui.dup.upgrade() {
                    let _ = d.hide();
                }
            });
        }
    }
    if let Some(d) = ui.countdown.upgrade() {
        {
            let ui = ui.clone();
            d.on_now(move || power_now(&ui));
        }
        {
            let ui = ui.clone();
            d.on_stop(move || {
                ui.power.borrow_mut().take();
                if let Some(d) = ui.countdown.upgrade() {
                    let _ = d.hide();
                }
            });
        }
        {
            let ui = ui.clone();
            d.window().on_close_requested(move || {
                ui.power.borrow_mut().take();
                slint::CloseRequestResponse::HideWindow
            });
        }
    }
}
