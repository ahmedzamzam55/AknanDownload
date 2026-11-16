use super::actions::delete_downloads;
use super::add_open::{open_add_dialog, show_progress_for_start};
use super::batch::open_batch;
use super::clipboard_ui::{is_http_url, paste_from_clipboard};
use super::look::apply_theme;
use super::main_menu::wire_main_menu;
use super::main_rows::wire_main_rows;
use super::modal::{confirm, modal_open};
use super::rows::refresh;
use super::selection::targets;
use super::state::{Ui, VideoExtra, APP};
use super::tray_sched::toggle_limiter;
use super::window::change_language;
use crate::i18n::t;
use crate::{clipboard, options, scheduler};
use slint::Timer;
use std::time::Duration;
use swiftget_engine::RequestHeaders;

pub(super) fn wire_main(ui: &Ui) {
    let Some(main) = ui.ctx.main.upgrade() else { return };
    let mgr = ui.ctx.mgr.clone();
    {
        let ctx = ui.ctx.clone();
        main.on_add_url(move || {
            let url = arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .and_then(|t| clipboard::downloadable_url(&t).or_else(|| is_http_url(&t)))
                .unwrap_or_default();
            open_add_dialog(&ctx, url, RequestHeaders::default(), None, VideoExtra::default());
        });
    }
    {
        let ui = ui.clone();
        main.on_add_batch(move || open_batch(&ui, String::new()));
    }
    {
        let ui = ui.clone();
        main.on_paste_url(move || paste_from_clipboard(&ui));
    }
    {
        let ui = ui.clone();
        main.on_refresh_list(move || refresh(&ui));
    }
    {
        let ui = ui.clone();
        main.on_add_batch_clipboard(move || {
            let text = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
            open_batch(&ui, text);
        });
    }
    {
        let ui = ui.clone();
        main.on_resume(move |id| {
            let targets = targets(&ui, id as u64);
            for &t in &targets {
                ui.ctx.mgr.start(t);
            }
            if targets.len() <= 8 {
                for &t in &targets {
                    show_progress_for_start(&ui, t);
                }
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_pause(move |id| {
            for t in targets(&ui, id as u64) {
                ui.ctx.mgr.pause(t);
            }
            refresh(&ui);
        });
    }
    {
        let mgr = mgr.clone();
        main.on_resume_all(move || mgr.start_all());
    }
    {
        let mgr = mgr.clone();
        main.on_stop_all(move || mgr.pause_all());
    }
    {
        let ui = ui.clone();
        main.on_delete(move |id| {
            // not inside the key press / click itself: the message box runs
            // its own message loop (holding Delete down asked many times)
            if modal_open() {
                return;
            }
            let ids = targets(&ui, id as u64);
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(1), move || {
                if !modal_open() {
                    delete_downloads(&ui, ids);
                    refresh(&ui);
                }
            });
        });
    }
    {
        let ui = ui.clone();
        main.on_delete_completed(move || {
            let ui2 = ui.clone();
            confirm(&ui.ctx, APP, &t("Remove every finished download from the list?\n(The files stay on the disk.)"), move || {
                ui2.ctx.mgr.remove_completed();
                refresh(&ui2);
            });
        });
    }
    {
        let ui = ui.clone();
        main.on_view_changed(move |_| refresh(&ui));
    }
    {
        let ui = ui.clone();
        main.on_search_changed(move || refresh(&ui));
    }
    {
        let ui = ui.clone();
        main.on_open_settings(move |tab| options::open(&ui, tab));
    }
    {
        let ui = ui.clone();
        main.on_open_limiter(move || options::open(&ui, 8));
    }
    {
        let ui = ui.clone();
        main.on_open_scheduler(move || scheduler::open(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_toggle_limiter(move || toggle_limiter(&ui));
    }
    {
        let ui = ui.clone();
        main.on_sort(move |key| {
            let mut s = ui.ctx.mgr.settings();
            if s.sort_by == key.as_str() {
                s.sort_desc = !s.sort_desc;
            } else {
                s.sort_by = key.to_string();
                s.sort_desc = matches!(key.as_str(), "date" | "size" | "added");
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_sort_by(s.sort_by.clone().into());
                m.set_sort_desc(s.sort_desc);
            }
            let _ = ui.ctx.mgr.set_settings(s);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_theme(move |m| {
            let mut s = ui.ctx.mgr.settings();
            s.theme = m.clamp(0, 12) as u32;
            let _ = ui.ctx.mgr.set_settings(s);
            apply_theme(&ui, m);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_language(move |lang| {
            let mut s = ui.ctx.mgr.settings();
            s.language = lang.to_string();
            let _ = ui.ctx.mgr.set_settings(s);
            change_language(&ui, &lang);
        });
    }
    {
        let ui = ui.clone();
        main.on_toggle_categories(move || {
            let mut s = ui.ctx.mgr.settings();
            s.hide_categories = !s.hide_categories;
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_hide_categories(s.hide_categories);
            }
            let _ = ui.ctx.mgr.set_settings(s);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_toolbar_small(move |small| {
            let mut s = ui.ctx.mgr.settings();
            s.toolbar_small = small;
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_toolbar_small(small);
            }
            let _ = ui.ctx.mgr.set_settings(s);
        });
    }
    wire_main_rows(&main, ui);
    wire_main_menu(&main, ui);
}
