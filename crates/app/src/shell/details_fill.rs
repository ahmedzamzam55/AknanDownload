use super::details::{close_progress, tray_tip};
use super::details_draw::{retain_speeds, ring_path, speed_drawing, taskbar_state};
use super::details_torrent::fill_torrent_tab;
use super::format::{err_text, kind_for, state_of, step_text};
use super::rows_fmt::update_model;
use super::state::Ui;
use crate::i18n::{t, tf};
use crate::{tray, winfx, DetailsDialog, SegView};
use slint::{ComponentHandle, SharedString, VecModel};
use swiftget_engine::{fmt_duration, fmt_size, DownloadKind, DownloadView, Status};

pub(super) fn update_progress_windows(ui: &Ui) {
    let ids: Vec<u64> = ui.progress.borrow().keys().copied().collect();
    retain_speeds(&ids);
    for id in ids {
        match ui.ctx.mgr.get(id) {
            // finished: the "complete" window takes over
            Some(v) if v.status == Status::Completed => close_progress(&ui.progress, id),
            Some(v) => {
                let (tip, progress) = tray_tip(&v);
                tray::update_download(id, &tip, progress);
                if let Some((d, segs)) = ui.progress.borrow().get(&id) {
                    fill_details(d, segs, &v);
                    if v.kind == DownloadKind::Torrent {
                        fill_torrent_tab(d, &ui.ctx.mgr, &v);
                    }
                    // also on the window's taskbar button (seen when it is minimized)
                    winfx::taskbar_progress(d.window(), taskbar_state(&v));
                }
            }
            None => close_progress(&ui.progress, id),
        }
    }
}

pub(super) fn fill_details(d: &DetailsDialog, model: &VecModel<SegView>, v: &DownloadView) {
    let pct = v.progress();
    d.set_name(v.file_name.clone().into());
    d.set_kind(if v.kind == DownloadKind::Torrent { "torrent" } else { kind_for(&v.file_name) }.into());
    d.set_percent(format!("{:.0}%", pct * 100.0).into());
    d.set_url(v.url.clone().into());
    let status = match v.status {
        _ if v.waiting => t("No internet connection - it will continue by itself when it is back…"),
        // a torrent: who it downloads from
        Status::Downloading | Status::Connecting if v.kind == DownloadKind::Torrent => {
            if v.active_connections == 0 {
                t("Looking for people who share this torrent…")
            } else {
                tf(
                    "Downloading from {} people ({} of them have the whole file)",
                    &[&v.active_connections.to_string(), &v.seeders.to_string()],
                )
            }
        }
        Status::Downloading => t("Receiving data…"),
        Status::Assembling => t("Joining the parts…"),
        Status::Failed => t("Error"),
        other => t(other.label()),
    };
    d.set_status(status.into());
    d.set_size(v.total_size.map(fmt_size).unwrap_or_else(|| t("Unknown")).into());
    let done = if v.parts_total > 0 {
        format!("{} ({:.1} % · {}/{})", fmt_size(v.downloaded), pct * 100.0, v.parts_done, v.parts_total)
    } else {
        format!("{} ({:.2} %)", fmt_size(v.downloaded), pct * 100.0)
    };
    d.set_downloaded(done.into());
    let speed = if v.speed > 0.0 { format!("{}/s", fmt_size(v.speed as u64)) } else { "—".into() };
    d.set_speed(speed.into());
    d.set_eta(v.eta_secs.map(fmt_duration).unwrap_or_else(|| "—".into()).into());
    d.set_resumable(t(if v.resumable { "Yes" } else { "No" }).into());
    d.set_resumable_ok(v.resumable);
    d.set_error(err_text(v.error.as_deref().unwrap_or_default()).into());
    d.set_progress(pct);
    // the 3D looks draw these
    d.set_ring_path(ring_path(pct as f64).into());
    let (line, area) = speed_drawing(v.id, v.speed);
    d.set_speed_line(line.into());
    d.set_speed_area(area.into());
    match v.step {
        Some((st, f)) => {
            d.set_step(format!("{}: {:.0}%", step_text(st), f * 100.0).into());
            d.set_step_progress(f);
            d.set_status(tf("{}…", &[&step_text(st)]).into());
        }
        None => {
            d.set_step(SharedString::new());
            d.set_step_progress(0.0);
        }
    }
    d.set_state(state_of(v.status));

    // connections, in file order
    let base = v
        .segments
        .iter()
        .filter(|s| s.2 != u64::MAX)
        .map(|s| s.2)
        .max()
        .unwrap_or(0);
    let mut segs: Vec<(u64, u64, u64, bool)> = v.segments.clone();
    segs.sort_by_key(|s| s.0);
    let rows: Vec<SegView> = if base == 0 {
        Vec::new()
    } else {
        segs.iter()
            .enumerate()
            .map(|(i, (start, pos, end, active))| SegView {
                start: *start as f32 / base as f32,
                done: *pos as f32 / base as f32,
                number: (i + 1).to_string().into(),
                downloaded: fmt_size(pos.saturating_sub(*start)).into(),
                info: if pos >= end {
                    t("Finished").into()
                } else if *active && v.status.is_running() {
                    t("Receiving data…").into()
                } else {
                    t("Waiting").into()
                },
                busy: pos < end && *active && v.status.is_running(),
            })
            .collect()
    };
    update_model(model, rows);
}
