// Aknan Download — desktop app.
// No console window in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

slint::include_modules!();

/// Bring a window in front of every other one (the browser too) for a
/// moment, then let it behave normally again.
#[macro_export]
macro_rules! to_front {
    ($w:expr) => {{
        let w = &$w;
        w.window().set_minimized(false);
        // drawn whole again: what Windows showed of it may be gone
        {
            use $crate::winfx::KeepDrawn as _;
            w.keep_drawn();
        }
        w.set_front(true);
        let weak = w.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(1800), move || {
            if let Some(w) = weak.upgrade() {
                w.set_front(false);
            }
        });
    }};
}

/// Open a window: the first time it is shown (again) it goes to
/// the middle of the screen and can't be maximized (only resized from its
/// edges); then it comes in front of the main window.
#[macro_export]
macro_rules! open_window {
    ($w:expr) => {{
        let was_open = $w.window().is_visible();
        if !was_open {
            // shown away from the screen, brought in once drawn
            $crate::winfx::send_away($w.window());
        }
        let _ = $w.show();
        if !was_open {
            $crate::winfx::place($w.as_weak(), false);
        }
        $crate::to_front!($w);
        $w.invoke_focus_input();
    }};
}

mod api;
mod assoc;
mod winmenu;
mod nativehost;
mod autostart;
mod browser;
mod cli;
mod clipboard;
mod clipui;
mod extra;
mod grab;
mod i18n;
mod options;
mod playlist;
mod preview;
mod torrent_ui;
mod scheduler;
mod sound;
mod tray;
mod update;
mod winfx;
mod shell;

pub(crate) use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::rc::Rc;
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use swiftget_engine::config::MAIN_QUEUE;
pub(crate) use swiftget_engine::filename;
pub(crate) use swiftget_engine::{
    fmt_size, AfterDone, DownloadKind, Manager, NewDownload, RequestHeaders, Settings, Status,
};

pub(crate) use shell::*;

fn main() {
    shell::bootstrap::entry();
}
