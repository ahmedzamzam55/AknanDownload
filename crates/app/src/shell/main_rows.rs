use super::actions::open_download;
use super::columns::{column_at, save_columns};
use super::details::open_progress;
use super::modal::message;
use super::row_menu::{row_action, row_menu};
use super::rows::refresh;
use super::selection::{multi_action, row_clicked, select_single, targets, visible_ids};
use super::shortcuts::SHORTCUTS;
use super::state::Ui;
use crate::i18n::t;
use crate::{extra, winfx, winmenu, MainWindow};
use slint::{ComponentHandle, Model};
use std::time::Duration;
use swiftget_engine::Status;

pub(super) fn wire_main_rows(main: &MainWindow, ui: &Ui) {
    {
        let ui = ui.clone();
        main.on_row_activated(move |id| {
            let Some(v) = ui.ctx.mgr.get(id as u64) else { return };
            if ui.ctx.mgr.settings().double_click == 1 {
                extra::open_properties(&ui, v.id);
            } else if v.status == Status::Completed {
                open_download(&v.save_path);
            } else {
                open_progress(&ui, v.id);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_row_action(move |id, action| {
            let targets = targets(&ui, id as u64);
            match action.as_str() {
                // these work on every selected download
                "resume" | "stop" | "restart" | "delete" | "copy" if targets.len() > 1 => {
                    multi_action(&ui, &targets, action.as_str())
                }
                a => row_action(&ui, id as u64, a),
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_to_queue(move |id, q| {
            for t in targets(&ui, id as u64) {
                ui.ctx.mgr.set_queue(t, (q > 0).then_some(q as u64));
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_move_selection(move |step, shift| {
            let order = visible_ids(&ui);
            let Some(m) = ui.ctx.main.upgrade() else { return };
            if order.is_empty() {
                return;
            }
            let at = order.iter().position(|i| *i as i32 == m.get_selected_id());
            let next = match (step, at) {
                (s, _) if s <= -9999 => 0,
                (s, _) if s >= 9999 => order.len() - 1,
                (s, None) if s < 0 => 0,
                (_, None) => order.len() - 1,
                (s, Some(i)) => (i as i64 + s as i64).clamp(0, order.len() as i64 - 1) as usize,
            };
            row_clicked(&ui, order[next], false, shift);
            refresh(&ui);
        });
    }
    main.on_show_shortcuts(|| message(&t(SHORTCUTS)));
    {
        let ui = ui.clone();
        main.on_row_clicked(move |id, ctrl, shift| {
            // Ctrl / Shift as Windows has them now (a menu or another window
            // may have kept the program from seeing one let go)
            let (ctrl, shift) = winfx::ctrl_shift_held().unwrap_or((ctrl, shift));
            row_clicked(&ui, id as u64, ctrl, shift);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_right(move |id| {
            if !ui.sel.borrow().ids.contains(&(id as u64)) {
                select_single(&ui, id as u64);
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_selected_id(id);
            }
            refresh(&ui);
        });
    }
    main.set_native_menu(winmenu::available());
    {
        let ui = ui.clone();
        main.on_row_menu(move |id| {
            // the newly selected row is drawn first (a Windows menu stops
            // the drawing of the program while it is open), then the menu
            let ui = ui.clone();
            if let Some(m) = ui.ctx.main.upgrade() {
                m.window().request_redraw();
            }
            slint::Timer::single_shot(Duration::from_millis(60), move || row_menu(&ui, id));
        });
    }
    {
        let ui = ui.clone();
        main.on_row_drag(move |id, offset| {
            // only while the left button is really held: Windows may keep the
            // release of a click that closed a menu, and the list must not
            // select rows when the mouse just moves
            if !winfx::left_button_down() {
                return;
            }
            // select with the mouse: from the row where the drag started
            let order = visible_ids(&ui);
            let Some(start) = order.iter().position(|x| *x == id as u64) else { return };
            let end = (start as i64 + offset as i64).clamp(0, order.len() as i64 - 1) as usize;
            let mut sel = ui.sel.borrow_mut();
            sel.ids = order[start.min(end)..=start.max(end)].to_vec();
            sel.anchor = Some(id as u64);
            drop(sel);
            refresh(&ui);
        });
    }
    {
        // the header: drag an edge to resize a column (live), let go to keep it
        let ui = ui.clone();
        main.on_column_resized(move |i, w| {
            if let Some(mut c) = ui.columns_model.row_data(i.max(0) as usize) {
                c.width = w.clamp(36.0, 2000.0);
                ui.columns_model.set_row_data(i as usize, c);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_columns_changed(move || save_columns(&ui));
    }
    {
        // the header: a title dragged onto another place moves the column there
        let ui = ui.clone();
        main.on_column_dropped(move |from, x| {
            let widths: Vec<f32> = (0..ui.columns_model.row_count()).filter_map(|i| ui.columns_model.row_data(i)).map(|c| c.width).collect();
            let to = column_at(&widths, x);
            let from = from.max(0) as usize;
            if from < widths.len() && to != from {
                if let Some(c) = ui.columns_model.row_data(from) {
                    ui.columns_model.remove(from);
                    ui.columns_model.insert(to.min(ui.columns_model.row_count()), c);
                    save_columns(&ui);
                }
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_row_drag_out(move |id| {
            if !winfx::left_button_down() {
                return;
            }
            // a finished file pulled out of the list: Windows takes it from here
            let Some(v) = ui.ctx.mgr.get(id as u64) else { return };
            if v.status != Status::Completed || !v.save_path.is_file() {
                return;
            }
            let Some(m) = ui.ctx.main.upgrade() else { return };
            winfx::drag_file_out(m.window(), &v.save_path);
            // the mouse was let go outside the list: the row must know too
            m.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: slint::LogicalPosition::new(0.0, 0.0),
                button: slint::platform::PointerEventButton::Left,
            });
            m.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
        });
    }
    {
        let ui = ui.clone();
        main.on_select_all(move || {
            let ids = visible_ids(&ui);
            if let Some(m) = ui.ctx.main.upgrade() {
                let cur = m.get_selected_id();
                if cur < 0 || !ids.iter().any(|&i| i as i32 == cur) {
                    m.set_selected_id(ids.first().map(|&i| i as i32).unwrap_or(-1));
                }
            }
            let mut sel = ui.sel.borrow_mut();
            sel.anchor = ids.first().copied();
            sel.ids = ids;
            drop(sel);
            refresh(&ui);
        });
    }
}
