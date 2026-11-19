//! Things Slint has no setting for, done with Windows itself:
//!
//! * window opacity (Options > General > Window opacity)
//! * every window opens in the middle of the screen; only the main window
//!   can be maximized (the others are resized from their edges)
//! * the progress of a download on its window's taskbar button
//! * dragging a finished file out of the list onto the desktop / a folder

mod dialogs;
mod drag;
#[cfg(windows)]
mod kbd_sync;
mod opacity;
mod placement;
mod redraw;
pub(crate) mod registry;
mod shortcuts;
mod taskbar;
#[cfg(test)]
mod tests;

pub(crate) use dialogs::{file_box, is_own_window_front, msg_box, on_top_of};
pub(crate) use drag::{drag_file_out, drag_paths_out};
#[cfg(windows)]
pub(crate) use opacity::hwnd;
pub(crate) use opacity::set_opacity;
#[cfg(windows)]
pub(crate) use placement::made_away;
pub(crate) use placement::{place, place_then, send_away};
pub(crate) use redraw::{keep_drawn_with_drop, KeepDrawn};
pub(crate) use shortcuts::{ctrl_shift_held, left_button_down};
pub(crate) use taskbar::{taskbar_progress, TaskbarProgress};
