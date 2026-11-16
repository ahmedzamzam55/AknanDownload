use super::{fill_list, open, select};
use crate::i18n::{t, tf};
use crate::*;
use swiftget_engine::config::{parse_hhmm, QueueDef};

/// The window's fields -> the queue definition.
fn read(d: &SchedulerDialog, mut q: QueueDef) -> Result<QueueDef, String> {
    let name = d.get_name().trim().to_string();
    if name.is_empty() {
        return Err(t("Give the queue a name"));
    }
    let start = d.get_start_time().trim().to_string();
    let stop = d.get_stop_time().trim().to_string();
    if d.get_start_enabled() && parse_hhmm(&start).is_none() {
        return Err(t("The start time must look like 02:30"));
    }
    if d.get_stop_enabled() && parse_hhmm(&stop).is_none() {
        return Err(t("The stop time must look like 08:00"));
    }
    let date = d.get_once_date().trim().to_string();
    if d.get_start_enabled() && d.get_once() && chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(t("The date must look like 2026-12-31"));
    }
    if q.id != MAIN_QUEUE {
        q.name = name;
    }
    q.schedule.start_enabled = d.get_start_enabled();
    q.schedule.start_time = start;
    q.schedule.once = d.get_once();
    q.schedule.once_date = date;
    q.schedule.days = [d.get_d0(), d.get_d1(), d.get_d2(), d.get_d3(), d.get_d4(), d.get_d5(), d.get_d6()];
    q.schedule.on_startup = d.get_on_startup();
    q.schedule.wake = d.get_wake();
    q.schedule.stop_enabled = d.get_stop_enabled();
    q.schedule.stop_time = stop;
    q.max_parallel = d.get_parallel().clamp(1, 32) as u32;
    q.retries = d.get_retries().clamp(0, 50) as u32;
    let folder = d.get_folder().trim().to_string();
    q.folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
    q.after_done = AfterDone::from_index(d.get_when_done());
    q.open_when_done = d.get_open_when_done();
    Ok(q)
}

pub(super) fn apply(ui: &Ui) -> bool {
    let Some(d) = ui.sched.upgrade() else { return false };
    let Some(id) = ui.sched_state.borrow().current else { return false };
    let Some(q) = ui.ctx.mgr.settings().queue(id).cloned() else { return false };
    match read(&d, q).map_err(anyhow::Error::msg).and_then(|q| ui.ctx.mgr.update_queue(q)) {
        Ok(()) => {
            d.set_message(SharedString::new());
            fill_list(ui, &d);
            refresh(ui);
            true
        }
        Err(e) => {
            d.set_message(format!("{e:#}").into());
            false
        }
    }
}

pub(crate) fn new_queue(ui: &Ui) {
    let s = ui.ctx.mgr.settings();
    let mut n = s.queues.len();
    let name = loop {
        let candidate = tf("Queue {}", &[&n]);
        if !s.queues.iter().any(|q| q.name == candidate) {
            break candidate;
        }
        n += 1;
    };
    match ui.ctx.mgr.add_queue(&name, 1, None) {
        Ok(id) => {
            open(ui, Some(id));
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_view(format!("queue:{id}").into());
            }
            refresh(ui);
        }
        Err(e) => message(&tf("Could not create the queue: {}", &[&format!("{e:#}")])),
    }
}

pub(crate) fn delete_queue(ui: &Ui, id: u64) {
    if id == MAIN_QUEUE {
        message(&t("The main download queue can't be deleted."));
        return;
    }
    let name = ui.ctx.mgr.settings().queue(id).map(|q| q.name.clone()).unwrap_or_default();
    if !ask(&tf("Delete the queue “{}”?\nIts files move to the main download queue.", &[&name])) {
        return;
    }
    match ui.ctx.mgr.delete_queue(id) {
        Ok(()) => {
            if let Some(m) = ui.ctx.main.upgrade() {
                if m.get_view() == format!("queue:{id}").as_str() {
                    m.set_view("all".into());
                }
            }
            if let Some(d) = ui.sched.upgrade() {
                if d.window().is_visible() {
                    fill_list(ui, &d);
                    select(ui, &d, MAIN_QUEUE);
                }
            }
            refresh(ui);
        }
        Err(e) => message(&format!("{e:#}")),
    }
}
