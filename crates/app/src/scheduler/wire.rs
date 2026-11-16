use super::actions::{apply, delete_queue, new_queue};
use super::{fill_files, select};
use crate::*;

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.sched.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_pick_queue(move |i| {
            let Some(d) = ui.sched.upgrade() else { return };
            let id = ui.sched_state.borrow().queue_ids.get(i.max(0) as usize).copied();
            if let Some(id) = id {
                select(&ui, &d, id);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_new_queue(move || new_queue(&ui));
    }
    {
        let ui = ui.clone();
        d.on_delete_queue(move || {
            if let Some(id) = ui.sched_state.borrow().current {
                delete_queue(&ui, id);
            }
        });
    }
    {
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = crate::winfx::file_box().set_directory(mgr.settings().download_dir).pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    let move_file = |ui: &Ui, delta: i32| {
        let Some(d) = ui.sched.upgrade() else { return };
        let i = d.get_file_current();
        let id = ui.sched_state.borrow().file_ids.get(i.max(0) as usize).copied();
        if let (true, Some(id)) = (i >= 0, id) {
            ui.ctx.mgr.move_in_queue(id, delta);
            fill_files(ui, &d);
            d.set_file_current(i + delta);
        }
    };
    {
        let ui = ui.clone();
        d.on_file_up(move || move_file(&ui, -1));
    }
    {
        let ui = ui.clone();
        d.on_file_down(move || move_file(&ui, 1));
    }
    {
        let ui = ui.clone();
        d.on_file_remove(move || {
            let Some(d) = ui.sched.upgrade() else { return };
            let i = d.get_file_current();
            let id = ui.sched_state.borrow().file_ids.get(i.max(0) as usize).copied();
            if let (true, Some(id)) = (i >= 0, id) {
                ui.ctx.mgr.set_queue(id, None);
                d.set_file_current(-1);
                fill_files(&ui, &d);
                refresh(&ui);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_start_now(move || {
            if apply(&ui) {
                if let Some(id) = ui.sched_state.borrow().current {
                    ui.ctx.mgr.start_queue(id);
                }
                refresh(&ui);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_stop_queue(move || {
            if let Some(id) = ui.sched_state.borrow().current {
                ui.ctx.mgr.stop_queue(id);
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_apply(move || {
            apply(&ui);
        });
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
