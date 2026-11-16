use super::format::{err_text, fmt_time, kind_for, state_of, step_text};
use crate::i18n::{self, t, tf};
use crate::DownloadRow;
use slint::{Model, VecModel};
use swiftget_engine::{fmt_duration, fmt_size, DownloadKind, DownloadView, Status};

pub(super) fn row_of(v: &DownloadView, sel: bool) -> DownloadRow {
    let pct = v.progress();
    let size = match v.total_size {
        Some(t) => fmt_size(t),
        None if v.downloaded > 0 => fmt_size(v.downloaded),
        None => String::new(),
    };
    let status = match v.status {
        _ if v.waiting => t("Waiting for connection…"),
        // after the data: joining, moving... with how far it is
        _ if v.step.is_some() => v.step.map(|(st, f)| format!("{} {:.0}%", step_text(st), f * 100.0)).unwrap_or_default(),
        Status::Downloading if v.kind == DownloadKind::Torrent && v.total_size.is_none() => t("Getting the file list…"),
        Status::Downloading | Status::Paused if v.total_size.is_some() => format!("{:.2}%", pct * 100.0),
        Status::Downloading if v.parts_total > 0 => tf("{}/{} parts", &[&v.parts_done, &v.parts_total]),
        Status::Downloading => fmt_size(v.downloaded),
        Status::Paused => String::new(),
        Status::Assembling => t("Merging…"),
        Status::Failed => t("Error"),
        other => t(other.label()),
    };
    let speed = if v.status.is_running() && v.speed > 0.0 {
        format!("{}/s", fmt_size(v.speed as u64))
    } else {
        String::new()
    };
    let description = match (&v.error, &v.description, v.kind) {
        (Some(e), _, _) if v.status == Status::Failed => err_text(e),
        (_, Some(d), _) => d.clone(),
        (_, _, DownloadKind::Dash) => tf("{}: video + audio", &[&t(&v.category)]),
        (_, _, DownloadKind::Hls) => tf("{}: stream", &[&t(&v.category)]),
        (_, _, DownloadKind::Torrent) if v.status.is_running() => tf("Torrent: {} peers", &[&v.active_connections]),
        (_, _, DownloadKind::Torrent) => t("Torrent"),
        _ => t(&v.category),
    };
    DownloadRow {
        id: v.id as i32,
        name: v.file_name.clone().into(),
        name_rtl: i18n::rtl_text(&v.file_name),
        kind: if v.kind == DownloadKind::Torrent { "torrent" } else { kind_for(&v.file_name) }.into(),
        category: v.category.clone().into(),
        queue: v.queue.map(|q| q.to_string()).unwrap_or_default().into(),
        size: size.into(),
        status: status.into(),
        progress: pct,
        eta: v.eta_secs.map(fmt_duration).unwrap_or_default().into(),
        speed: speed.into(),
        last_try: fmt_time(v.finished_at.or(v.last_try).unwrap_or(v.added_at)).into(),
        description: description.into(),
        state: state_of(v.status),
        added: fmt_time(v.added_at).into(),
        sel,
        save_to: v.save_path.parent().map(|p| p.display().to_string()).unwrap_or_default().into(),
        referer: v.referer().to_string().into(),
        parent: v.parent_page.clone().unwrap_or_default().into(),
    }
}

/// Does a download belong to the sidebar item that is selected?
pub(super) fn in_view(v: &DownloadView, view: &str) -> bool {
    match view {
        "all" => true,
        "unfinished" => v.status != Status::Completed,
        "finished" => v.status == Status::Completed,
        other => {
            if let Some(c) = other.strip_prefix("cat:") {
                v.category == c
            } else if let Some(c) = other.strip_prefix("unf:") {
                v.category == c && v.status != Status::Completed
            } else if let Some(c) = other.strip_prefix("fin:") {
                v.category == c && v.status == Status::Completed
            } else if let Some(q) = other.strip_prefix("queue:") {
                v.queue.map(|x| x.to_string()).as_deref() == Some(q)
            } else {
                true
            }
        }
    }
}

pub(super) fn sort_views(views: &mut [DownloadView], sort_by: &str, sort_desc: bool) {
    // sorting (header click / View > Arrange files)
    let order = |v: &DownloadView| state_of(v.status);
    match sort_by {
        "name" => views.sort_by_key(|v| v.file_name.to_lowercase()),
        "size" => views.sort_by_key(|v| v.total_size.unwrap_or(v.downloaded)),
        "status" => views.sort_by_key(|v| (order(v), v.id)),
        "eta" => views.sort_by_key(|v| (v.eta_secs.unwrap_or(u64::MAX), v.id)),
        "speed" => views.sort_by_key(|v| (v.speed as u64, v.id)),
        "desc" => views.sort_by_key(|v| (v.description.clone().unwrap_or_default().to_lowercase(), v.id)),
        "saveto" => views.sort_by_key(|v| (v.save_path.parent().map(|p| p.to_path_buf()), v.id)),
        "referer" => views.sort_by_key(|v| (v.referer().to_lowercase(), v.id)),
        "parent" => views.sort_by_key(|v| (v.parent_page.clone().unwrap_or_default().to_lowercase(), v.id)),
        "added" => views.sort_by_key(|v| (v.added_at, v.id)),
        _ => views.sort_by_key(|v| (v.finished_at.or(v.last_try).unwrap_or(v.added_at), v.id)),
    }
    if sort_desc {
        views.reverse();
    }
}

pub(crate) fn update_model<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() == rows.len() {
        for (i, r) in rows.into_iter().enumerate() {
            if model.row_data(i).as_ref() != Some(&r) {
                model.set_row_data(i, r);
            }
        }
    } else {
        model.set_vec(rows);
    }
}
