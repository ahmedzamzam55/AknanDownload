use super::state::Ui;
use crate::i18n::{t, tf};
use crate::{DetailsDialog, PieceRun, TFileRow, TPeerRow};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use swiftget_engine::{fmt_size, DownloadView, Manager};

/// The "Torrent" tab of a torrent's window: its files (each can be left out
/// while it runs), people, trackers (one can be added), its pieces, check
/// again, its magnet link.
pub(super) fn wire_torrent_tab(ui: &Ui, d: &DetailsDialog, id: u64, v: &DownloadView) {
    d.set_is_torrent(true);
    d.set_t_files(ModelRc::new(VecModel::<TFileRow>::default()));
    d.set_t_peers(ModelRc::new(VecModel::<TPeerRow>::default()));
    d.set_t_trackers(ModelRc::new(VecModel::<SharedString>::default()));
    d.set_t_pieces(ModelRc::new(VecModel::<PieceRun>::default()));
    d.set_t_first_last(ui.ctx.mgr.torrent_first_last(id));
    let _ = v;
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_t_file_toggled(move |i| {
            let Some(d) = w.upgrade() else { return };
            let files = d.get_t_files();
            let Some(m) = files.as_any().downcast_ref::<VecModel<TFileRow>>() else { return };
            let Some(mut row) = m.row_data(i as usize) else { return };
            row.selected = !row.selected;
            m.set_row_data(i as usize, row);
            let rows: Vec<TFileRow> = m.iter().collect();
            if !rows.iter().any(|r| r.selected) {
                // at least one file
                if let Some(mut r) = m.row_data(i as usize) {
                    r.selected = true;
                    m.set_row_data(i as usize, r);
                }
                d.set_t_note(t("Check at least one file").into());
                return;
            }
            let select = (!rows.iter().all(|r| r.selected))
                .then(|| rows.iter().filter(|r| r.selected).map(|r| r.index.to_string()).collect::<Vec<_>>().join(","));
            mgr.set_torrent_files(id, select);
            d.set_t_note(t("The torrent goes on with the files checked.").into());
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_t_add_tracker(move || {
            let Some(d) = w.upgrade() else { return };
            let tr = d.get_t_new_tracker().trim().to_string();
            if mgr.add_torrent_tracker(id, &tr) {
                d.set_t_new_tracker(SharedString::new());
                d.set_t_note(t("Tracker added.").into());
            } else {
                d.set_t_note(t("This is not a tracker address (udp://, http:// or https://).").into());
            }
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_t_first_last_changed(move || {
            if let Some(d) = w.upgrade() {
                mgr.set_torrent_first_last(id, d.get_t_first_last());
            }
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_t_recheck(move || {
            mgr.recheck_torrent(id);
            if let Some(d) = w.upgrade() {
                d.set_t_note(t("Every piece on the disk is checked again, then it goes on.").into());
            }
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_t_copy_magnet(move || {
            let Some(d) = w.upgrade() else { return };
            match mgr.magnet_link(id) {
                Some(m) => {
                    if let Ok(mut c) = arboard::Clipboard::new() {
                        let _ = c.set_text(m);
                    }
                    d.set_t_note(t("The magnet link was copied.").into());
                }
                None => d.set_t_note(t("The magnet link could not be made.").into()),
            }
        });
    }
}

/// The Torrent tab's lists, as the torrent is now (rows changed in place,
/// so the lists keep where they were scrolled to).
pub(super) fn fill_torrent_tab(d: &DetailsDialog, mgr: &Manager, v: &DownloadView) {
    let Some(live) = mgr.torrent_live(v.id) else { return };
    fn put<T: Clone + PartialEq + 'static>(model: ModelRc<T>, rows: Vec<T>) {
        let Some(m) = model.as_any().downcast_ref::<VecModel<T>>() else { return };
        if m.row_count() != rows.len() {
            m.set_vec(rows);
            return;
        }
        for (i, r) in rows.into_iter().enumerate() {
            if m.row_data(i).as_ref() != Some(&r) {
                m.set_row_data(i, r);
            }
        }
    }
    let base = v.save_path.clone();
    let files: Vec<TFileRow> = live
        .files
        .iter()
        .map(|f| TFileRow {
            index: f.index as i32,
            name: f.path.strip_prefix(base.parent().unwrap_or(&base)).unwrap_or(&f.path).display().to_string().into(),
            size: fmt_size(f.size).into(),
            done: if f.size > 0 { (f.done.min(f.size) as f64 / f.size as f64) as f32 } else { 0.0 },
            selected: f.selected,
        })
        .collect();
    put(d.get_t_files(), files);
    let peers: Vec<TPeerRow> = live
        .peers
        .iter()
        .map(|p| TPeerRow {
            address: p.address.clone().into(),
            down: format!("{}/s", fmt_size(p.down)).into(),
            up: format!("{}/s", fmt_size(p.up)).into(),
            // (not known of every person: not shown)
            have: if p.have < 0.0 { SharedString::new() } else { format!("{:.0}%", p.have * 100.0).into() },
            seeder: p.seeder,
        })
        .collect();
    put(d.get_t_peers(), peers);
    put(d.get_t_trackers(), live.trackers.iter().map(|t| SharedString::from(t.as_str())).collect());
    put(d.get_t_pieces(), live.pieces.iter().map(|(a, b, h)| PieceRun { from: *a, to: *b, here: *h }).collect());
    let ratio = if v.downloaded > 0 { live.uploaded as f64 / v.downloaded as f64 } else { 0.0 };
    d.set_t_summary(tf("Sent to others: {} (ratio {})", &[&fmt_size(live.uploaded), &format!("{ratio:.2}")]).into());
}
