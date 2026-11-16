use super::actions::delete_downloads;
use super::add_open::show_progress_for_start;
use super::modal::{ask, modal_open};
use super::rows::refresh;
use super::state::Ui;
use crate::clipboard;
use crate::i18n::tf;
use slint::{Model, Timer};
use std::time::Duration;

/// Ids in the order the list shows them.
pub(super) fn visible_ids(ui: &Ui) -> Vec<u64> {
    (0..ui.rows.row_count()).filter_map(|i| ui.rows.row_data(i)).map(|r| r.id as u64).collect()
}

pub(crate) fn select_single(ui: &Ui, id: u64) {
    let mut sel = ui.sel.borrow_mut();
    sel.ids = vec![id];
    sel.anchor = Some(id);
    drop(sel);
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_selected_id(id as i32);
    }
}

/// Click on a row: alone, Ctrl = add / remove, Shift = range from the last click.
pub(super) fn row_clicked(ui: &Ui, id: u64, ctrl: bool, shift: bool) {
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_selected_id(id as i32);
    }
    let order = visible_ids(ui);
    let mut sel = ui.sel.borrow_mut();
    if shift {
        let anchor = sel.anchor.filter(|a| order.contains(a)).unwrap_or(id);
        let (a, b) = (
            order.iter().position(|x| *x == anchor).unwrap_or(0),
            order.iter().position(|x| *x == id).unwrap_or(0),
        );
        let range: Vec<u64> = order[a.min(b)..=a.max(b)].to_vec();
        if ctrl {
            for r in range {
                if !sel.ids.contains(&r) {
                    sel.ids.push(r);
                }
            }
        } else {
            sel.ids = range;
        }
    } else if ctrl {
        if let Some(p) = sel.ids.iter().position(|x| *x == id) {
            sel.ids.remove(p);
        } else {
            sel.ids.push(id);
        }
        sel.anchor = Some(id);
    } else {
        sel.ids = vec![id];
        sel.anchor = Some(id);
    }
}

/// The downloads an action works on: the whole selection when the clicked
/// one is part of it, otherwise only that one.
pub(super) fn targets(ui: &Ui, id: u64) -> Vec<u64> {
    let sel = ui.sel.borrow();
    if id as i64 >= 0 && sel.ids.contains(&id) && sel.ids.len() > 1 {
        let order = visible_ids(ui);
        let mut v: Vec<u64> = order.into_iter().filter(|x| sel.ids.contains(x)).collect();
        if v.is_empty() {
            v = sel.ids.clone();
        }
        v
    } else if (id as i64) >= 0 {
        vec![id]
    } else {
        Vec::new()
    }
}

/// Resume / stop / redownload / delete / copy several downloads at once.
pub(super) fn multi_action(ui: &Ui, ids: &[u64], action: &str) {
    match action {
        "resume" => {
            for &i in ids.iter() {
                ui.ctx.mgr.start(i);
            }
            // a progress window for each (not for a long list at once)
            if ids.len() <= 8 {
                for &i in ids.iter() {
                    show_progress_for_start(ui, i);
                }
            }
        }
        "stop" => ids.iter().for_each(|&i| ui.ctx.mgr.pause(i)),
        "restart" => {
            if ask(&tf("Download these {} files again from the beginning?", &[&ids.len()])) {
                ids.iter().for_each(|&i| ui.ctx.mgr.restart(i));
            }
        }
        "delete" => {
            if modal_open() {
                return;
            }
            let (ui, ids) = (ui.clone(), ids.to_vec());
            Timer::single_shot(Duration::from_millis(1), move || {
                if !modal_open() {
                    delete_downloads(&ui, ids);
                    refresh(&ui);
                }
            });
        }
        "copy" => {
            let text: Vec<String> = ids.iter().filter_map(|&i| ui.ctx.mgr.get(i)).map(|v| v.url).collect();
            let joined = text.join("\r\n");
            if !joined.is_empty() {
                clipboard::ignore_text(&joined);
                if let Ok(mut c) = arboard::Clipboard::new() {
                    let _ = c.set_text(joined);
                }
            }
        }
        _ => {}
    }
}
