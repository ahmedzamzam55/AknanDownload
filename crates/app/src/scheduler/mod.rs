//! The Scheduler window: every queue with its schedule, its files (in
//! download order) and its options.

mod actions;
mod wire;

pub(crate) use actions::{delete_queue, new_queue};
pub(crate) use wire::wire;

use crate::i18n::t;
use crate::*;

#[derive(Default)]
pub(crate) struct SchedState {
    /// queue ids in the window's list
    pub(super) queue_ids: Vec<u64>,
    pub(super) current: Option<u64>,
    /// download ids of the "Files in the queue" list
    pub(super) file_ids: Vec<u64>,
}

fn queue_rows(ui: &Ui) -> Vec<QueueRow> {
    let s = ui.ctx.mgr.settings();
    let views = ui.ctx.mgr.snapshot();
    s.queues
        .iter()
        .map(|q| QueueRow {
            id: q.id as i32,
            name: crate::queue_title(q).into(),
            count: views.iter().filter(|v| v.queue == Some(q.id) && v.status != Status::Completed).count() as i32,
            running: ui.ctx.mgr.queue_running(q.id),
        })
        .collect()
}

/// Open the window on a queue (None = the one selected in the sidebar).
pub(crate) fn open(ui: &Ui, queue: Option<u64>) {
    let Some(d) = ui.sched.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    let wanted = queue
        .or_else(|| ui.ctx.main.upgrade().map(|m| m.get_current_queue() as u64))
        .filter(|q| s.queue(*q).is_some())
        .unwrap_or(MAIN_QUEUE);
    fill_list(ui, &d);
    select(ui, &d, wanted);
    d.set_message(SharedString::new());
    open_window!(d);
    d.window().set_minimized(false);
}

pub(super) fn fill_list(ui: &Ui, d: &SchedulerDialog) {
    let rows = queue_rows(ui);
    ui.sched_state.borrow_mut().queue_ids = rows.iter().map(|r| r.id as u64).collect();
    d.set_queues(ModelRc::new(VecModel::from(rows)));
}

pub(super) fn select(ui: &Ui, d: &SchedulerDialog, id: u64) {
    let s = ui.ctx.mgr.settings();
    let Some(q) = s.queue(id).cloned() else { return };
    let index = ui.sched_state.borrow().queue_ids.iter().position(|x| *x == id).unwrap_or(0);
    ui.sched_state.borrow_mut().current = Some(id);
    d.set_current(index as i32);
    d.set_is_main(id == MAIN_QUEUE);
    d.set_name(crate::queue_title(&q).into());
    let sc = &q.schedule;
    d.set_start_enabled(sc.start_enabled);
    d.set_start_time(sc.start_time.clone().into());
    d.set_once(sc.once);
    d.set_once_date(if sc.once_date.is_empty() {
        chrono::Local::now().format("%Y-%m-%d").to_string().into()
    } else {
        sc.once_date.clone().into()
    });
    d.set_d0(sc.days[0]);
    d.set_d1(sc.days[1]);
    d.set_d2(sc.days[2]);
    d.set_d3(sc.days[3]);
    d.set_d4(sc.days[4]);
    d.set_d5(sc.days[5]);
    d.set_d6(sc.days[6]);
    d.set_on_startup(sc.on_startup);
    d.set_wake(sc.wake);
    d.set_stop_enabled(sc.stop_enabled);
    d.set_stop_time(sc.stop_time.clone().into());
    d.set_parallel(q.max_parallel.clamp(1, 32) as i32);
    d.set_retries(q.retries.min(50) as i32);
    d.set_folder(q.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default().into());
    d.set_when_done(q.after_done.index());
    d.set_open_when_done(q.open_when_done);
    d.set_file_current(-1);
    fill_files(ui, d);
}

pub(super) fn fill_files(ui: &Ui, d: &SchedulerDialog) {
    let Some(q) = ui.sched_state.borrow().current else { return };
    let files = ui.ctx.mgr.queue_files(q);
    let rows: Vec<PairRow> = files
        .iter()
        .map(|v| PairRow {
            a: v.file_name.clone().into(),
            b: v.total_size.map(fmt_size).unwrap_or_default().into(),
            c: match v.status {
                Status::Downloading | Status::Paused if v.total_size.is_some() => {
                    format!("{:.1}%  {}", v.progress() * 100.0, if v.status == Status::Paused { "" } else { "▶" })
                }
                other => t(other.label()),
            }
            .into(),
        })
        .collect();
    let ids: Vec<u64> = files.iter().map(|v| v.id).collect();
    let changed = ui.sched_state.borrow().file_ids != ids || d.get_files().row_count() != rows.len();
    ui.sched_state.borrow_mut().file_ids = ids;
    if changed {
        d.set_files(ModelRc::new(VecModel::from(rows)));
    } else if let Some(m) = d.get_files().as_any().downcast_ref::<VecModel<PairRow>>() {
        update_model(m, rows);
    } else {
        d.set_files(ModelRc::new(VecModel::from(rows)));
    }
    d.set_running(ui.ctx.mgr.queue_running(q));
}

/// Keep the running marks and the files list current while the window is open.
pub(crate) fn poll(ui: &Ui) {
    let Some(d) = ui.sched.upgrade() else { return };
    if !d.window().is_visible() {
        return;
    }
    let rows = queue_rows(ui);
    if let Some(m) = d.get_queues().as_any().downcast_ref::<VecModel<QueueRow>>() {
        if m.row_count() == rows.len() {
            update_model(m, rows);
        }
    }
    fill_files(ui, &d);
}
