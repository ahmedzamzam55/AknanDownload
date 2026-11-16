use super::look::{apply_look, Look};
use super::modal::ask;
use super::rows::refresh;
use super::state::{Ctx, Ui};
use crate::i18n::{self, t};
use crate::winfx;
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::Manager;

pub(crate) fn bring_to_front(ctx: &Ctx) {
    if let Some(w) = ctx.main.upgrade() {
        // the main window keeps its maximize button (open_window! is for the others)
        let was_open = w.window().is_visible();
        if !was_open {
            winfx::send_away(w.window());
        }
        let _ = w.show();
        if !was_open {
            winfx::place(w.as_weak(), true);
        }
        to_front!(w);
        w.invoke_focus_list();
    }
}

// ------------------------------------------------------------ window sizes
// The size the user gave a window is kept (Options don't have it: it is
// dragged), and the window opens with it next time.

thread_local! {
    // the size each window had at the last look (saved once it stays the same)
    static SEEN_SIZE: RefCell<HashMap<String, (f32, f32)>> = RefCell::new(HashMap::new());
}

/// The size a window had when the program was used last.
pub(crate) fn saved_size(mgr: &Manager, key: &str) -> Option<(f32, f32)> {
    mgr.settings()
        .window_sizes
        .get(key)
        .copied()
        .filter(|&(w, h)| (200.0..10000.0).contains(&w) && (80.0..10000.0).contains(&h))
}

/// The size of `w` now, in the units the windows are made with.
fn size_now(w: &slint::Window) -> Option<(f32, f32)> {
    if !w.is_visible() || w.is_minimized() || w.is_maximized() {
        return None;
    }
    let s = w.size().to_logical(w.scale_factor());
    Some((s.width.round(), s.height.round()))
}

/// Called about once a second: a size that stayed the same since the last
/// look and is new is saved (not while the edge is still being dragged).
pub(crate) fn follow_size(mgr: &Arc<Manager>, key: &str, size: Option<(f32, f32)>) {
    let Some(size) = size else { return };
    let steady = SEEN_SIZE.with(|m| m.borrow_mut().insert(key.to_string(), size) == Some(size));
    if steady && mgr.settings().window_sizes.get(key) != Some(&size) {
        let mut s = mgr.settings();
        s.window_sizes.insert(key.to_string(), size);
        let _ = mgr.set_settings(s);
    }
}

/// The sizes of the main window and the progress windows, once a second.
pub(super) fn follow_sizes(ui: &Ui) {
    if let Some(m) = ui.ctx.main.upgrade() {
        follow_size(&ui.ctx.mgr, "main", size_now(m.window()));
    }
    for (d, _) in ui.progress.borrow().values() {
        // the height counts only with the details shown (without them the
        // window is as small as it can be)
        if let Some((w, h)) = size_now(d.window()) {
            let h = if d.get_show_details() {
                h
            } else {
                saved_size(&ui.ctx.mgr, "progress").map(|s| s.1).unwrap_or(0.0)
            };
            follow_size(&ui.ctx.mgr, "progress", Some((w, h)));
        }
    }
}

/// Switch the whole program to another language right away.
pub(crate) fn set_language(ui: &Ui, lang: &str) {
    i18n::apply(lang);
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_language(lang.into());
    }
    // every window reads from the side of the language
    if let Some(t) = ui.tray.borrow().as_ref() {
        t.refresh_labels();
    }
    apply_look(ui, &Look::of(&ui.ctx.mgr.settings()));
    refresh(ui);
}

/// Set when the program should open again after it closes (the windows
/// change their reading direction).
static RESTART: AtomicBool = AtomicBool::new(false);

pub(super) fn should_restart() -> bool {
    RESTART.load(Ordering::SeqCst)
}

/// The language was changed by the user: when the windows now read from the
/// other side, the program opens again so everything starts that way.
pub(crate) fn change_language(ui: &Ui, lang: &str) {
    let was_rtl = i18n::is_rtl();
    set_language(ui, lang);
    if was_rtl != i18n::is_rtl() {
        // after the menu / Options window is gone
        Timer::single_shot(Duration::from_millis(200), move || {
            if ask(&t("Aknan Download will restart now to show its windows in the direction of the new language. Downloads continue after it opens.\n\nRestart now?")) {
                RESTART.store(true, Ordering::SeqCst);
                let _ = slint::quit_event_loop();
            }
        });
    }
}

/// A setting needs a new start: asked (after the window that changed it
/// is gone); yes = everything is saved, it closes and opens again.
pub(crate) fn ask_restart(text: String) {
    Timer::single_shot(Duration::from_millis(200), move || {
        if ask(&text) {
            RESTART.store(true, Ordering::SeqCst);
            let _ = slint::quit_event_loop();
        }
    });
}

/// Open the program again (after it saved everything and closed).
pub(super) fn restart_now() {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--restarted");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS: not tied to this one
        cmd.creation_flags(0x0000_0008);
    }
    let _ = cmd.spawn();
}
