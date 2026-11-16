use super::add_open::open_add_dialog;
use super::rows::refresh;
use super::shortcuts::about;
use super::state::{Ui, VideoExtra};
use super::window::bring_to_front;
use crate::{options, scheduler, tray};
use slint::{ComponentHandle, Timer};
use std::cell::Cell;
use std::time::{Duration, Instant};
use swiftget_engine::RequestHeaders;

pub(super) fn tray_action(ui: &Ui, action: tray::TrayAction) {
    use tray::TrayAction as T;
    // a click waits a moment: the second click of a double click is not
    // taken for "go away"
    thread_local! {
        static CLICKS: Cell<u32> = const { Cell::new(0) };
        static SHOWN_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    }
    match action {
        T::Show => {
            CLICKS.with(|c| c.set(c.get() + 1));
            SHOWN_AT.with(|t| t.set(Some(Instant::now())));
            bring_to_front(&ui.ctx);
        }
        T::Toggle => {
            // the last click of a double click (Windows may send it after the double click)
            if SHOWN_AT.with(|t| t.get()).is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
                return;
            }
            let n = CLICKS.with(|c| {
                c.set(c.get() + 1);
                c.get()
            });
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(320), move || {
                if CLICKS.with(|c| c.get()) != n {
                    return;
                }
                let Some(w) = ui.ctx.main.upgrade() else { return };
                let shown = w.window().is_visible() && !w.window().is_minimized();
                if shown {
                    // open already: to the tray (or the taskbar)
                    if ui.ctx.mgr.settings().close_to_tray {
                        let _ = w.hide();
                    } else {
                        w.window().set_minimized(true);
                    }
                } else {
                    w.window().set_minimized(false);
                    bring_to_front(&ui.ctx);
                }
            });
        }
        T::AddUrl => {
            bring_to_front(&ui.ctx);
            open_add_dialog(&ui.ctx, String::new(), RequestHeaders::default(), None, VideoExtra::default());
        }
        T::ResumeAll => ui.ctx.mgr.start_all(),
        T::StopAll => ui.ctx.mgr.pause_all(),
        T::ToggleLimiter => toggle_limiter(ui),
        T::Scheduler => scheduler::open(ui, None),
        T::Options => options::open(ui, 0),
        T::About => about(),
        T::Exit => {
            let _ = slint::quit_event_loop();
        }
    }
}

pub(super) fn toggle_limiter(ui: &Ui) {
    let mut s = ui.ctx.mgr.settings();
    s.speed_limit_on = !s.speed_limit_on;
    if s.speed_limit_kb == 0 {
        s.speed_limit_kb = 500;
    }
    let _ = ui.ctx.mgr.set_settings(s);
    refresh(ui);
}
