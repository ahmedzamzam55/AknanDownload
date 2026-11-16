use super::menubar_build::build_menu;
use super::state::Ui;
use crate::{i18n, winfx, winmenu, MainWindow, MenuData, Theme};
use slint::{ComponentHandle, Model};
use std::time::Duration;

pub(super) fn open_top_menu(ui: &Ui, idx: i32, x: f32, y: f32) {
    let Some(main) = ui.ctx.main.upgrade() else { return };
    let items = build_menu(ui, &main, idx);
    if items.is_empty() {
        return;
    }
    main.set_active_menu(idx);
    let dark = main.global::<Theme>().get_dark();
    let picked = winmenu::popup_at(main.window(), &items, x, y, dark, i18n::is_rtl());
    main.set_active_menu(-1);
    for wait in [150, 450] {
        let weak = ui.ctx.main.clone();
        slint::Timer::single_shot(Duration::from_millis(wait), move || {
            if let Some(m) = weak.upgrade() {
                if !winfx::left_button_down() {
                    let _ = m.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerExited);
                }
            }
        });
    }
    if let Some(picked) = picked {
        dispatch(ui, &main, picked);
    }
}

fn dispatch(ui: &Ui, main: &MainWindow, id: u32) {
    let data = main.global::<MenuData>();
    let sel = main.get_selected_id();
    match id {
        100..=149 => if let Some(m) = data.get_tasks_main().row_data((id - 100) as usize) { main.invoke_act(m.id); },
        150..=159 => if let Some(m) = data.get_export_items().row_data((id - 150) as usize) { main.invoke_export_list(m.id); },
        160..=169 => if let Some(m) = data.get_import_items().row_data((id - 160) as usize) { main.invoke_import_list(m.id); },
        170..=179 => if let Some(m) = data.get_tasks_end().row_data((id - 170) as usize) { main.invoke_act(m.id); },
        200..=219 => if let Some(m) = data.get_file_top().row_data((id - 200) as usize) {
            if matches!(m.id.as_str(), "restart" | "refresh") { main.invoke_row_action(sel, m.id); } else { main.invoke_act(m.id); }
        },
        220..=239 => if let Some(m) = data.get_file_mid().row_data((id - 220) as usize) { main.invoke_row_action(sel, m.id); },
        240 => main.invoke_select_all(),
        241 => main.invoke_row_action(sel, "properties".into()),
        300..=319 => if let Some(m) = data.get_downloads_top().row_data((id - 300) as usize) { main.invoke_act(m.id); },
        320..=339 => if let Some(m) = data.get_downloads_find().row_data((id - 320) as usize) { main.invoke_act(m.id); },
        340 => if !main.get_limiter_on() { main.invoke_toggle_limiter(); },
        341 => if main.get_limiter_on() { main.invoke_toggle_limiter(); },
        342 => main.invoke_open_limiter(),
        343 => main.invoke_open_scheduler(),
        344 => main.invoke_open_settings(0),
        400 => main.invoke_toggle_categories(),
        401 => main.invoke_set_toolbar_small(false),
        402 => main.invoke_set_toolbar_small(true),
        403 => main.invoke_open_columns(),
        404 => main.invoke_open_settings(9),
        500..=519 => if let Some(m) = data.get_help_items().row_data((id - 500) as usize) { main.invoke_act(m.id); },
        520 => main.invoke_about(),
        1000..=1999 => if let Some(q) = ui.queues.row_data((id - 1000) as usize) { main.invoke_start_queue(q.id); },
        2000..=2999 => if let Some(q) = ui.queues.row_data((id - 2000) as usize) { main.invoke_stop_queue(q.id); },
        3000..=3999 => if let Some(d) = data.get_all_done_items().row_data((id - 3000) as usize) { main.invoke_set_all_done(d.val); },
        4000..=4999 => if let Some(a) = data.get_arrange_items().row_data((id - 4000) as usize) { main.invoke_sort(a.key); },
        5000..=5999 => if let Some(l) = data.get_lang_items().row_data((id - 5000) as usize) { main.invoke_set_language(l.key); },
        6000..=6999 => main.invoke_set_theme((id - 6000) as i32),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::menubar_build::file_enabled;

    #[test]
    fn file_menu_item_enabled_states() {
        assert!(!file_enabled("resume", -1, -1, false, false));
        assert!(file_enabled("resume", 1, 2, true, false));
        assert!(file_enabled("pause", 1, 0, false, true));
        assert!(!file_enabled("pause", 1, 3, false, false));
        assert!(!file_enabled("open", 1, 0, false, false));
        assert!(file_enabled("open", 1, 3, false, false));
        assert!(file_enabled("openwith", 1, 3, false, false));
        assert!(file_enabled("refresh", 1, 2, false, false));
        assert!(!file_enabled("refresh", 1, 3, false, false));
        assert!(!file_enabled("preview", -1, 0, false, false));
        assert!(file_enabled("folder", 1, 3, false, false));
        assert!(!file_enabled("folder", -1, 0, false, false));
    }
}
