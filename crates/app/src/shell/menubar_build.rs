use super::state::Ui;
use crate::winmenu::Item;
use crate::{MainWindow, MenuAction, MenuData, MenuLabels, Theme};
use slint::{ComponentHandle, Model, ModelRc};

pub(super) const fn file_enabled(id: &str, sel: i32, st: i32, can_resume: bool, can_stop: bool) -> bool {
    match id.as_bytes() {
        b"resume" => can_resume,
        b"pause" => can_stop,
        b"refresh" | b"preview" => sel >= 0 && st != 3,
        b"open" | b"openwith" => st == 3,
        _ => sel >= 0,
    }
}

fn push_actions(dst: &mut Vec<Item>, model: ModelRc<MenuAction>, base: u32) {
    for (i, m) in model.iter().enumerate() {
        dst.push(Item::new(base + i as u32, m.title.to_string(), true));
    }
}

pub(super) fn build_menu(ui: &Ui, main: &MainWindow, idx: i32) -> Vec<Item> {
    let data = main.global::<MenuData>();
    let labels = main.global::<MenuLabels>();
    let mut out = Vec::new();
    match idx {
        0 => build_tasks(&data, &labels, &mut out),
        1 => build_file(main, &data, &labels, &mut out),
        2 => build_downloads(ui, main, &data, &labels, &mut out),
        3 => build_view(main, &data, &labels, &mut out),
        4 => build_help(&data, &labels, &mut out),
        _ => {}
    }
    out
}

fn build_tasks(data: &MenuData<'_>, labels: &MenuLabels<'_>, out: &mut Vec<Item>) {
    push_actions(out, data.get_tasks_main(), 100);
    out.push(Item::separator());
    let mut ex = Vec::new();
    push_actions(&mut ex, data.get_export_items(), 150);
    out.push(Item { sub: ex, ..Item::new(0, labels.get_export_menu().to_string(), true) });
    let mut im = Vec::new();
    push_actions(&mut im, data.get_import_items(), 160);
    out.push(Item { sub: im, ..Item::new(0, labels.get_import_menu().to_string(), true) });
    out.push(Item::separator());
    push_actions(out, data.get_tasks_end(), 170);
}

fn build_file(main: &MainWindow, data: &MenuData<'_>, labels: &MenuLabels<'_>, out: &mut Vec<Item>) {
    let (sel, st, cr, cs) = (
        main.get_selected_id(),
        main.get_selected_state(),
        main.get_can_resume(),
        main.get_can_stop(),
    );
    for (i, m) in data.get_file_top().iter().enumerate() {
        out.push(Item::new(200 + i as u32, m.title.to_string(), file_enabled(m.id.as_str(), sel, st, cr, cs)));
    }
    out.push(Item::separator());
    for (i, m) in data.get_file_mid().iter().enumerate() {
        out.push(Item::new(220 + i as u32, m.title.to_string(), file_enabled(m.id.as_str(), sel, st, cr, cs)));
    }
    out.push(Item::separator());
    out.push(Item::new(240, labels.get_select_all().to_string(), true));
    out.push(Item::new(241, labels.get_properties().to_string(), sel >= 0));
}

fn build_downloads(ui: &Ui, main: &MainWindow, data: &MenuData<'_>, labels: &MenuLabels<'_>, out: &mut Vec<Item>) {
    push_actions(out, data.get_downloads_top(), 300);
    out.push(Item::separator());
    push_actions(out, data.get_downloads_find(), 320);
    out.push(Item::separator());
    let mut sq = Vec::new();
    let mut pq = Vec::new();
    for i in 0..ui.queues.row_count() {
        if let Some(q) = ui.queues.row_data(i) {
            sq.push(Item::new(1000 + i as u32, q.name.to_string(), true));
            pq.push(Item::new(2000 + i as u32, q.name.to_string(), true));
        }
    }
    out.push(Item { sub: sq, ..Item::new(0, labels.get_start_queue().to_string(), true) });
    out.push(Item { sub: pq, ..Item::new(0, labels.get_stop_queue().to_string(), true) });
    let on = main.get_limiter_on();
    let lim = vec![
        Item { checked: on, ..Item::new(340, labels.get_turn_on().to_string(), true) },
        Item { checked: !on, ..Item::new(341, labels.get_turn_off().to_string(), true) },
        Item::separator(),
        Item::new(342, labels.get_settings().to_string(), true),
    ];
    out.push(Item { sub: lim, ..Item::new(0, labels.get_speed_limiter().to_string(), true) });
    let done = main.get_all_done();
    let all_done: Vec<Item> = data
        .get_all_done_items()
        .iter()
        .enumerate()
        .map(|(i, d)| Item { checked: d.val == done, ..Item::new(3000 + i as u32, d.title.to_string(), true) })
        .collect();
    out.push(Item { sub: all_done, ..Item::new(0, labels.get_when_all_done().to_string(), true) });
    out.push(Item::separator());
    out.push(Item::new(343, labels.get_scheduler().to_string(), true));
    out.push(Item::new(344, labels.get_options().to_string(), true));
}

fn build_view(main: &MainWindow, data: &MenuData<'_>, labels: &MenuLabels<'_>, out: &mut Vec<Item>) {
    out.push(Item { checked: !main.get_hide_categories(), ..Item::new(400, labels.get_show_categories().to_string(), true) });
    let sort_by = main.get_sort_by();
    let arr: Vec<Item> = data
        .get_arrange_items()
        .iter()
        .enumerate()
        .map(|(i, a)| Item { checked: a.key == sort_by, ..Item::new(4000 + i as u32, a.title.to_string(), true) })
        .collect();
    out.push(Item { sub: arr, ..Item::new(0, labels.get_arrange_files().to_string(), true) });
    let small = main.get_toolbar_small();
    let tb = vec![
        Item { checked: !small, ..Item::new(401, labels.get_large_buttons().to_string(), true) },
        Item { checked: small, ..Item::new(402, labels.get_small_buttons().to_string(), true) },
    ];
    out.push(Item { sub: tb, ..Item::new(0, labels.get_toolbar().to_string(), true) });
    out.push(Item::new(403, labels.get_columns().to_string(), true));
    let lang = main.get_language();
    let langs: Vec<Item> = data
        .get_lang_items()
        .iter()
        .enumerate()
        .map(|(i, l)| Item { checked: l.key == lang, ..Item::new(5000 + i as u32, l.title.to_string(), true) })
        .collect();
    out.push(Item { sub: langs, ..Item::new(0, labels.get_language().to_string(), true) });
    let mode = main.global::<Theme>().get_mode();
    let mut themes: Vec<Item> = data
        .get_theme_items()
        .iter()
        .enumerate()
        .map(|(i, t)| Item { checked: i as i32 == mode, ..Item::new(6000 + i as u32, t.to_string(), true) })
        .collect();
    themes.push(Item::separator());
    themes.push(Item::new(404, labels.get_appearance().to_string(), true));
    out.push(Item { sub: themes, ..Item::new(0, labels.get_theme().to_string(), true) });
}

fn build_help(data: &MenuData<'_>, labels: &MenuLabels<'_>, out: &mut Vec<Item>) {
    push_actions(out, data.get_help_items(), 500);
    out.push(Item::separator());
    out.push(Item::new(520, labels.get_about().to_string(), true));
}
