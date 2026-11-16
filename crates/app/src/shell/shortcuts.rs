use super::look::{set_theme_of, Look};
use crate::{AboutDialog, Theme};
use slint::{ComponentHandle, Weak};
use std::cell::RefCell;

/// The keys of the list (F1 shows them).
pub(crate) const SHORTCUTS: &str = "Keyboard shortcuts\n\n\
Ctrl+N or Ctrl+V (Shift+Insert): add a download (from clipboard)\n\
Ctrl+B: add several addresses\n\
Ctrl+T: add a torrent\n\
Ctrl+G: grab links from a page\n\
Ctrl+P: playlist / channel\n\
Ctrl+C (Ctrl+Insert): copy the address (or selected text)\n\
Space: resume / pause\n\
Ctrl+R / Ctrl+Shift+R: resume / resume all\n\
Ctrl+S / Ctrl+Shift+S: stop / stop all\n\
Ctrl+O / Ctrl+Shift+O: open the file / open completed\n\
Ctrl+E: open its folder\n\
Ctrl+J: open downloads folder\n\
Ctrl+D: download details\n\
Enter / Alt+Enter: open download / properties\n\
F2: rename / move the file\n\
Delete: delete\n\
Ctrl+A: select all\n\
Ctrl+F, Ctrl+K / Ctrl+L, F3: find, search box, find next\n\
F5 / Escape: refresh / close window or clear search\n\
Ctrl+, / Ctrl+Q / F1: settings / quit / shortcuts\n\
Up / Down, PageUp / PageDown, Home / End: move in list (with Shift: select)";

thread_local! {
    static ABOUT: RefCell<Option<Weak<AboutDialog>>> = const { RefCell::new(None) };
}

pub(super) fn wire_about(about_dlg: &AboutDialog) {
    about_dlg.on_open_link(|u| {
        if u.starts_with("https://") {
            let _ = open::that_detached(u.as_str());
        }
    });
    let w = about_dlg.as_weak();
    about_dlg.on_close_window(move || {
        if let Some(d) = w.upgrade() {
            let _ = d.hide();
        }
    });
    ABOUT.with(|a| *a.borrow_mut() = Some(about_dlg.as_weak()));
}

pub(super) fn about() {
    ABOUT.with(|a| {
        if let Some(d) = a.borrow().as_ref().and_then(|w| w.upgrade()) {
            d.set_version(env!("CARGO_PKG_VERSION").into());
            open_window!(d);
        }
    });
}

pub(super) fn style_about(look: &Look) {
    ABOUT.with(|a| {
        if let Some(d) = a.borrow().as_ref().and_then(|w| w.upgrade()) {
            set_theme_of(d.global::<Theme>(), look);
        }
    });
}
