use crate::i18n::{t, tf};
use crate::*;

pub(crate) fn open_find(ui: &Ui) {
    if let Some(d) = ui.find.upgrade() {
        d.set_message(SharedString::new());
        open_window!(d);
    }
}

/// Select the next download (after the selected one) that matches.
pub(crate) fn find_next(ui: &Ui) {
    let (Some(d), Some(m)) = (ui.find.upgrade(), ui.ctx.main.upgrade()) else { return };
    let needle = d.get_text().to_string();
    if needle.trim().is_empty() {
        open_find(ui);
        return;
    }
    let case = d.get_match_case();
    let whole = d.get_whole();
    let norm = |s: &str| if case { s.to_string() } else { s.to_lowercase() };
    let needle = norm(needle.trim());
    let hit = |hay: &str| {
        let hay = norm(hay);
        if whole {
            hay == needle
        } else {
            hay.contains(&needle)
        }
    };
    // search in the whole list, in the order shown
    if m.get_view() != "all" && m.get_search().is_empty() {
        m.set_view("all".into());
        refresh(ui);
    }
    let ids: Vec<u64> = (0..ui.rows.row_count()).filter_map(|i| ui.rows.row_data(i)).map(|r| r.id as u64).collect();
    let start = ids.iter().position(|id| *id as i32 == m.get_selected_id()).map(|p| p + 1).unwrap_or(0);
    for k in 0..ids.len() {
        let id = ids[(start + k) % ids.len()];
        let Some(v) = ui.ctx.mgr.get(id) else { continue };
        let found = (d.get_in_name() && hit(&v.file_name))
            || (d.get_in_desc() && hit(v.description.as_deref().unwrap_or("")))
            || (d.get_in_address()
                && (hit(&v.url) || hit(v.referer()) || hit(v.parent_page.as_deref().unwrap_or(""))));
        if found {
            select_single(ui, id);
            d.set_message(tf("Found: {}", &[&v.file_name]).into());
            refresh(ui);
            return;
        }
    }
    d.set_message(t("Not found").into());
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.find.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_find_next(move || find_next(&ui));
    }
    {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}
