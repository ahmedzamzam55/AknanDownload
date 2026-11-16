use super::add::early_download;
use super::add_open::open_add_dialog;
use super::complete::{show_complete, show_group_complete};
use super::details::open_progress;
use super::details_fill::update_progress_windows;
use super::events::handle_events;
use super::modal::modal_open;
use super::rows::refresh;
use super::state::{Ctx, Ui, VideoExtra};
use super::tray_sched::tray_action;
use super::window::follow_sizes;
use crate::{api, clipboard, extra, grab, scheduler, tray, winfx};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{Manager, NewDownload, RequestHeaders};

pub(super) fn spawn_clipboard(mgr: &Arc<Manager>, ctx: &Ctx) {
    let enabled_mgr = mgr.clone();
    let ctx = ctx.clone();
    let types_mgr = mgr.clone();
    clipboard::spawn(
        move || enabled_mgr.settings().clipboard_monitor,
        move || {
            types_mgr
                .settings()
                .capture_exts
                .split_whitespace()
                .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty() && !e.contains('*'))
                .collect()
        },
        move |url| {
            let ctx = ctx.clone();
            let _ = slint::invoke_from_event_loop(move || {
                // a torrent always asks first (it also shares with others)
                if ctx.mgr.settings().auto_start_clipboard && !swiftget_engine::torrent::is_magnet(&url) {
                    if ctx.mgr.find_url(&url).is_none() {
                        ctx.mgr.add(NewDownload { url, start: true, ..Default::default() });
                    }
                    return;
                }
                let busy = ctx.add.upgrade().map(|d| d.window().is_visible()).unwrap_or(true);
                if !busy {
                    open_add_dialog(&ctx, url, RequestHeaders::default(), None, VideoExtra::default());
                }
            });
        },
    );
}

pub(super) fn start_timers(ui: &Ui) -> (Timer, Timer, Timer) {
    // progress windows: 10 times a second, so bars and numbers move smoothly
    let fast_timer = Timer::default();
    {
        let ui = ui.clone();
        let ticks = Cell::new(0u32);
        fast_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if modal_open() {
                return;
            }
            if !ui.progress.borrow().is_empty() {
                update_progress_windows(&ui);
            }
            grab::poll(&ui);
            ticks.set(ticks.get().wrapping_add(1));
            let tray_actions = ui
                .tray
                .borrow()
                .as_ref()
                .map(|t| {
                    // the check mark only needs to follow about once a second
                    if ticks.get().is_multiple_of(10) {
                        t.set_limiter(ui.ctx.mgr.settings().speed_limit_on);
                    }
                    t.poll()
                })
                .unwrap_or_default();
            for action in tray_actions {
                tray_action(&ui, action);
            }
            for (id, action) in tray::take_download_actions() {
                match action {
                    tray::DownloadAction::Show => open_progress(&ui, id),
                    tray::DownloadAction::Pause => ui.ctx.mgr.pause(id),
                }
            }
        });
    }
    // the list: a few times a second
    let timer = Timer::default();
    {
        let ui = ui.clone();
        let hidden_ticks = Cell::new(0u32);
        timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            // the windows are alive (a second copy started by the user asks)
            api::UI_ALIVE.store(api::now_secs(), Ordering::Relaxed);
            if modal_open() {
                return;
            }
            // a file picker that was open is closed again
            api::UI_WAITS.store(false, Ordering::Relaxed);
            // next to the clock (window hidden): the list is brought up to
            // date only every few seconds, nobody looks at it
            let shown = ui.ctx.main.upgrade().map(|m| m.window().is_visible() && !m.window().is_minimized()).unwrap_or(false);
            hidden_ticks.set(if shown { 0 } else { hidden_ticks.get().wrapping_add(1) });
            if shown || hidden_ticks.get() % 12 == 1 {
                refresh(&ui);
            }
            scheduler::poll(&ui);

            // the download behind an open "Download File Info" says nothing
            // until Start is pressed
            let early = early_download(&ui.ctx);
            let completed = ui.ctx.mgr.take_completed();
            extra::finished_well(&ui, completed.len());
            extra::av1_tip(&ui, &completed);
            let finished: Vec<u64> = completed.into_iter().filter(|id| Some(*id) != early).collect();
            let s = ui.ctx.mgr.settings();
            // added together with others (a playlist, a batch): one window
            // once all of them are done, not one for every file
            let mut single = None;
            for id in &finished {
                match ui.ctx.mgr.get(*id).and_then(|v| v.group).filter(|_| !s.complete_each_in_group) {
                    Some(g) => {
                        ui.groups_waiting.borrow_mut().insert(g);
                    }
                    None => single = Some(*id),
                }
            }
            if let Some(last) = single {
                if s.show_complete_dialog {
                    show_complete(&ui, last);
                }
            }
            let waiting: Vec<u64> = ui.groups_waiting.borrow().iter().copied().collect();
            for g in waiting {
                let (all, done, failed, busy) = ui.ctx.mgr.group_state(g);
                if !busy {
                    ui.groups_waiting.borrow_mut().remove(&g);
                    if all > 0 && s.show_complete_dialog {
                        show_group_complete(&ui, g, all, done, failed);
                    }
                }
            }
            handle_events(&ui);
            if let Some(action) = ui.ctx.mgr.take_power_request() {
                extra::start_countdown(&ui, action);
            }
            if ui.ctx.mgr.take_exit_request() {
                let _ = slint::quit_event_loop();
            }
        });
    }
    // shut down / sleep countdown: once a second
    let power_timer = Timer::default();
    {
        let ui = ui.clone();
        power_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            extra::tick_countdown(&ui);
            follow_sizes(&ui);
            // the window may have been shown again from the icon next to the clock
            if let Some(m) = ui.ctx.main.upgrade() {
                if m.window().is_visible() {
                    winfx::set_opacity(m.window(), ui.ctx.mgr.settings().opacity);
                }
            }
        });
    }
    (fast_timer, timer, power_timer)
}
