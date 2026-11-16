use super::add::early_download;
use super::format::{completion_index, kind_of_category, queue_title, state_of};
use super::rows_fmt::{in_view, row_of, sort_views, update_model};
use super::state::Ui;
use crate::i18n::{t, tf};
use crate::{CatRow, DownloadRow, QueueRow};
use slint::SharedString;
use swiftget_engine::config::MAIN_QUEUE;
use swiftget_engine::{fmt_size, DownloadView, Status};

pub(crate) fn refresh(ui: &Ui) {
    let Some(w) = ui.ctx.main.upgrade() else { return };
    let mut views = ui.ctx.mgr.snapshot();
    // not in the list before Start is pressed in "Download File Info"
    if let Some(early) = early_download(&ui.ctx) {
        views.retain(|v| v.id != early);
    }
    let settings = ui.ctx.mgr.settings();
    let view = w.get_view().to_string();
    let search = w.get_search().to_lowercase();

    // the queue the toolbar's Start/Stop Queue work on
    let current_queue = view
        .strip_prefix("queue:")
        .and_then(|q| q.parse::<u64>().ok())
        .filter(|q| settings.queue(*q).is_some())
        .unwrap_or(MAIN_QUEUE);
    w.set_current_queue(current_queue as i32);

    sort_views(&mut views, settings.sort_by.as_str(), settings.sort_desc);

    let mut new_rows: Vec<DownloadRow> = views
        .iter()
        .filter(|v| in_view(v, &view))
        .filter(|v| {
            search.is_empty()
                || v.file_name.to_lowercase().contains(&search)
                || v.url.to_lowercase().contains(&search)
                || v.description.as_deref().unwrap_or("").to_lowercase().contains(&search)
        })
        .map(|v| row_of(v, false))
        .collect();
    // the selection: drop rows that are gone, mark the rest
    {
        let mut sel = ui.sel.borrow_mut();
        sel.ids.retain(|id| views.iter().any(|v| v.id == *id));
        let focus = w.get_selected_id();
        if sel.ids.is_empty() && focus >= 0 && views.iter().any(|v| v.id as i32 == focus) {
            sel.ids.push(focus as u64);
        }
        for r in new_rows.iter_mut() {
            r.sel = sel.ids.contains(&(r.id as u64));
        }
        let chosen: Vec<&DownloadView> = views.iter().filter(|v| sel.ids.contains(&v.id)).collect();
        w.set_sel_count(chosen.len() as i32);
        w.set_can_resume(chosen.iter().any(|v| matches!(v.status, Status::Paused | Status::Failed)));
        w.set_can_stop(chosen.iter().any(|v| v.status == Status::Queued || v.status.is_running()));
    }
    update_model(&ui.rows, new_rows);

    // sidebar: categories (General last) with their counts
    let count = |f: &dyn Fn(&DownloadView) -> bool| {
        let n = views.iter().filter(|v| f(v)).count();
        if n > 0 {
            SharedString::from(n.to_string())
        } else {
            SharedString::new()
        }
    };
    let builtin: Vec<String> = settings.categories.iter().filter(|c| c.builtin).map(|c| c.name.clone()).collect();
    let cat_rows: Vec<CatRow> = settings
        .category_names()
        .into_iter()
        .map(|name| CatRow {
            label: t(&name).into(),
            kind: kind_of_category(&name).into(),
            count: count(&|v| v.category == name),
            unfinished: count(&|v| v.category == name && v.status != Status::Completed),
            finished: count(&|v| v.category == name && v.status == Status::Completed),
            builtin: name == "General" || builtin.contains(&name),
            name: name.into(),
        })
        .collect();
    update_model(&ui.cats, cat_rows);
    w.set_count_all(count(&|_| true));
    w.set_count_unfinished(count(&|v| v.status != Status::Completed));
    w.set_count_finished(count(&|v| v.status == Status::Completed));

    let queue_rows: Vec<QueueRow> = settings
        .queues
        .iter()
        .map(|q| QueueRow {
            id: q.id as i32,
            name: queue_title(q).into(),
            count: views
                .iter()
                .filter(|v| v.queue == Some(q.id) && v.status != Status::Completed)
                .count() as i32,
            running: views.iter().any(|v| v.queue == Some(q.id) && v.status.is_running()),
        })
        .collect();
    update_model(&ui.queues, queue_rows);

    let sel = w.get_selected_id();
    let sel_state = views
        .iter()
        .find(|v| v.id as i32 == sel)
        .map(|v| state_of(v.status))
        .unwrap_or(-1);
    if sel_state == -1 && sel >= 0 {
        w.set_selected_id(-1);
        ui.sel.borrow_mut().ids.clear();
    }
    w.set_selected_state(sel_state);

    let running = views.iter().filter(|v| v.status.is_running()).count();
    let speed: f64 = views.iter().map(|v| v.speed).sum();
    let text = if running > 0 {
        tf("{} downloading  ·  {}/s", &[&running, &fmt_size(speed as u64)])
    } else {
        tf("Ready  ·  {} files in the list", &[&views.len()])
    };
    w.set_status_text(text.into());
    w.set_limiter_on(settings.speed_limit_on);
    let limiter_text = if settings.speed_limit_on {
        tf("Speed limiter: on ({} KB/s)", &[&settings.speed_limit_kb])
    } else {
        t("Speed limiter: off")
    };
    w.set_limiter_text(limiter_text.into());
    w.set_all_done(completion_index(ui.ctx.mgr.when_all_done()));
}
