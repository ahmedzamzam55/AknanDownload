use super::shortcuts::style_about;
use super::state::Ui;
use crate::{i18n, winmenu, Theme};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::Path;
use swiftget_engine::Settings;

/// Fonts of Options > Appearance > Font ("" = Windows' own).
pub(crate) const FONTS: [&str; 7] = ["", "Tahoma", "Arial", "Calibri", "Verdana", "Times New Roman", "Courier New"];

/// Everything that decides how the windows look.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Look {
    pub mode: i32,
    /// 0.8 ..= 1.5
    pub scale: f32,
    pub family: String,
    /// picture behind the main window ("" = none)
    pub background: String,
    /// 0.1 ..= 0.8: how much of the picture shows through
    pub strength: f32,
}

impl Look {
    pub(crate) fn of(s: &Settings) -> Look {
        Look {
            mode: s.theme.min(12) as i32,
            scale: s.font_scale.clamp(80, 150) as f32 / 100.0,
            family: if FONTS.contains(&s.font_family.as_str()) { s.font_family.clone() } else { String::new() },
            background: s.background_image.clone(),
            strength: s.background_strength.clamp(10, 80) as f32 / 100.0,
        }
    }
}

thread_local! {
    // the picture behind the main window, loaded once per file
    static BACKGROUND: RefCell<(String, Option<slint::Image>)> = const { RefCell::new((String::new(), None)) };
}

/// The picture of `path` (a png / jpg the user chose); None when it can't be read.
fn background_image(path: &str) -> Option<slint::Image> {
    if path.trim().is_empty() {
        return None;
    }
    BACKGROUND.with(|b| {
        let mut b = b.borrow_mut();
        if b.0 != path {
            // a picture is at most a few MB; anything huge is not loaded
            let ok = std::fs::metadata(path).map(|m| m.is_file() && m.len() <= 40 << 20).unwrap_or(false);
            *b = (path.to_string(), if ok { slint::Image::load_from_path(Path::new(path)).ok() } else { None });
        }
        b.1.clone()
    })
}

/// Set the theme on every open window (each window has its own copy).
pub(crate) fn apply_theme(ui: &Ui, mode: i32) {
    let mut look = Look::of(&ui.ctx.mgr.settings());
    look.mode = mode;
    apply_look(ui, &look);
}

pub(crate) fn set_theme_of(t: Theme<'_>, look: &Look) {
    t.set_rtl(i18n::is_rtl());
    t.set_mode(look.mode);
    t.set_font_scale(look.scale);
    t.set_font_family(look.family.clone().into());
}

/// Show `look` on every window right away.
pub(crate) fn apply_look(ui: &Ui, look: &Look) {
    macro_rules! set {
        ($($w:expr),*) => { $( if let Some(w) = $w.upgrade() { set_theme_of(w.global::<Theme>(), look); } )* };
    }
    set!(
        ui.ctx.main,
        ui.ctx.add,
        ui.batch,
        ui.settings_dlg,
        ui.complete,
        ui.columns,
        ui.category,
        ui.find,
        ui.props,
        ui.dup,
        ui.countdown,
        ui.sched,
        ui.grabber,
        ui.tips,
        ui.refresh_dlg,
        ui.playlist,
        ui.torrent,
        ui.browsers,
        ui.updater
    );
    style_about(look);
    for (d, _) in ui.progress.borrow().values() {
        set_theme_of(d.global::<Theme>(), look);
    }
    // the program's Windows menus too (the menu bar's lists, the tray menu,
    // the list's right-click menu): dark with a dark look
    if let Some(m) = ui.ctx.main.upgrade() {
        winmenu::set_dark_menus(m.global::<Theme>().get_dark());
    }
    // the main window only: the picture behind it
    if let Some(m) = ui.ctx.main.upgrade() {
        match background_image(&look.background) {
            Some(img) => {
                m.set_bg_image(img);
                m.set_has_bg_image(true);
                m.global::<Theme>().set_glass(look.strength);
            }
            None => {
                m.set_has_bg_image(false);
                m.global::<Theme>().set_glass(0.0);
            }
        }
    }
}
