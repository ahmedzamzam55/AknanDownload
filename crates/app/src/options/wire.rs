use super::lists::wire_lists;
use super::read::read;
use crate::i18n::{t, tf};
use crate::*;
use swiftget_engine::config::DEFAULT_CAPTURE_EXTS;

/// The look the Options window shows now (not saved yet).
fn look_of(d: &SettingsDialog) -> crate::Look {
    crate::Look {
        mode: d.get_theme().clamp(0, 12),
        scale: (d.get_text_size().round() as u32).clamp(80, 150) as f32 / 100.0,
        family: crate::FONTS.get(d.get_font_index().max(0) as usize).copied().unwrap_or("").to_string(),
        background: d.get_bg_image().to_string(),
        strength: (d.get_bg_strength().round() as u32).clamp(10, 80) as f32 / 100.0,
    }
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.settings_dlg.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_preview_look(move || {
            if let Some(d) = ui.settings_dlg.upgrade() {
                crate::apply_look(&ui, &look_of(&d));
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_browse_bg(move || {
            let Some(d) = ui.settings_dlg.upgrade() else { return };
            let pick = crate::winfx::file_box().add_filter(t("Pictures"), &["png", "jpg", "jpeg"]).pick_file();
            if let Some(f) = pick {
                d.set_bg_image(f.display().to_string().into());
                crate::apply_look(&ui, &look_of(&d));
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_preview_opacity(move |v| {
            if let Some(m) = ui.ctx.main.upgrade() {
                crate::winfx::set_opacity(m.window(), v.round() as u32);
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_browse(move |which| {
            let Some(d) = w.upgrade() else { return };
            match which.as_str() {
                "download" => {
                    let start = PathBuf::from(d.get_download_dir().as_str());
                    if let Some(dir) = crate::winfx::file_box().set_directory(start).pick_folder() {
                        d.set_download_dir(dir.display().to_string().into());
                    }
                }
                "temp" => {
                    if let Some(dir) = crate::winfx::file_box().pick_folder() {
                        d.set_temp_dir(dir.display().to_string().into());
                    }
                }
                "virus" => {
                    if let Some(f) = crate::winfx::file_box().add_filter(t("Program"), &["exe", "com", "bat", "cmd"]).pick_file() {
                        d.set_virus_program(f.display().to_string().into());
                    }
                }
                s if s.starts_with("sound:") => {
                    let mut dlg = crate::winfx::file_box().add_filter(t("Sound"), &["wav"]);
                    if cfg!(windows) {
                        dlg = dlg.set_directory(r"C:\Windows\Media");
                    }
                    if let Some(f) = dlg.pick_file() {
                        let p: SharedString = f.display().to_string().into();
                        match &s[6..] {
                            "complete" => d.set_sound_complete(p),
                            "failed" => d.set_sound_failed(p),
                            "qstart" => d.set_sound_qstart(p),
                            _ => d.set_sound_qdone(p),
                        }
                    }
                }
                _ => {}
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_play(move |which| {
            let Some(d) = w.upgrade() else { return };
            let p = match which.as_str() {
                "complete" => d.get_sound_complete(),
                "failed" => d.get_sound_failed(),
                "qstart" => d.get_sound_qstart(),
                _ => d.get_sound_qdone(),
            };
            sound::play(&p);
        });
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_reset_paths(move || {
            let mut s = mgr.settings();
            s.category_dirs.clear();
            let _ = mgr.set_settings(s);
            message(&t("The remembered category folders were cleared."));
        });
    }
    {
        let w = d.as_weak();
        d.on_default_exts(move || {
            if let Some(d) = w.upgrade() {
                d.set_capture_exts(DEFAULT_CAPTURE_EXTS.into());
            }
        });
    }
    wire_lists(ui, &d);
    {
        let ui2 = ui.clone();
        d.on_open_extension_folder(move || browser::open(&ui2));
    }
    {
        let ui2 = ui.clone();
        d.on_save(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let old = ui2.ctx.mgr.settings();
            match read(&ui2, &d, old.clone()) {
                Ok(s) => {
                    let port_changed = s.browser_port != old.browser_port;
                    // settings that only take effect at the next start
                    let drawing_changed = s.gpu_drawing != old.gpu_drawing;
                    let theme = s.theme as i32;
                    let torrents_changed = s.torrent_main != old.torrent_main;
                    let view = s.clone();
                    if let Err(e) = ui2.ctx.mgr.set_settings(s) {
                        d.set_message(tf("Could not save: {}", &[&format!("{e:#}")]).into());
                        return;
                    }
                    apply_theme(&ui2, theme);
                    apply_view_settings(&ui2, &view);
                    if torrents_changed {
                        crate::assoc::torrents(view.torrent_main);
                    }
                    crate::change_language(&ui2, crate::i18n::effective(&view.language));
                    let _ = d.hide();
                    refresh(&ui2);
                    if port_changed || drawing_changed {
                        let text = if port_changed {
                            t("Aknan Download needs to restart to use the new port (set the same port in the browser add-in too). Downloads continue after it opens.\n\nRestart now?")
                        } else {
                            t("Aknan Download needs to restart to draw its windows the new way. Downloads continue after it opens.\n\nRestart now?")
                        };
                        crate::ask_restart(text);
                    }
                }
                Err(msg) => d.set_message(msg.into()),
            }
        });
    }
    {
        let w = d.as_weak();
        let ui = ui.clone();
        d.on_cancel(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
            // the sliders only showed it: back to the saved look
            if let Some(m) = ui.ctx.main.upgrade() {
                crate::winfx::set_opacity(m.window(), ui.ctx.mgr.settings().opacity);
            }
            crate::apply_look(&ui, &crate::Look::of(&ui.ctx.mgr.settings()));
        });
    }
}
