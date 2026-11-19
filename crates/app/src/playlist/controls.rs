use super::{dedup, subs, sync_rows_meta, update_checked_text, PlaylistState, PLAYLIST_RETRIES, QUALITIES};
use crate::i18n::t;
use crate::*;
use swiftget_engine::sync::LockOk;

pub(super) fn wire_controls(
    d: &PlaylistDialog,
    rows: &Rc<VecModel<LinkRow>>,
    state: &Arc<Mutex<PlaylistState>>,
    ui: &Ui,
) {
    {
        let (w, r, st) = (d.as_weak(), rows.clone(), state.clone());
        d.on_toggle(move |i| {
            if let Some(mut row) = r.row_data(i.max(0) as usize) {
                row.checked = !row.checked;
                r.set_row_data(i as usize, row);
            }
            if let Some(d) = w.upgrade() {
                update_checked_text(&r, &d, &st.lock_ok());
            }
        });
    }
    {
        let (w, r, st) = (d.as_weak(), rows.clone(), state.clone());
        d.on_check_all(move |on| {
            for i in 0..r.row_count() {
                if let Some(mut row) = r.row_data(i) {
                    row.checked = on;
                    r.set_row_data(i, row);
                }
            }
            if let Some(d) = w.upgrade() {
                update_checked_text(&r, &d, &st.lock_ok());
            }
        });
    }
    {
        let (w, r, st) = (d.as_weak(), rows.clone(), state.clone());
        d.on_quality_changed(move || {
            if let Some(d) = w.upgrade() {
                sync_rows_meta(&r, &d, &st.lock_ok());
            }
        });
    }
    {
        let (w, r, st) = (d.as_weak(), rows.clone(), state.clone());
        d.on_order_changed(move || {
            if let Some(d) = w.upgrade() {
                sync_rows_meta(&r, &d, &st.lock_ok());
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = crate::winfx::file_box().pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let (ui2, st) = (ui.clone(), state.clone());
        d.on_download(move || start_download(&ui2, &st));
    }
    {
        let (w, st) = (d.as_weak(), state.clone());
        d.on_close_window(move || {
            st.lock_ok().cancel_bg();
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
    {
        let (w, st) = (d.as_weak(), state.clone());
        d.window().on_close_requested(move || {
            st.lock_ok().cancel_bg();
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
}

fn start_download(ui: &Ui, state: &Arc<Mutex<PlaylistState>>) {
    let Some(d) = ui.playlist.upgrade() else { return };
    let chosen: Vec<LinkRow> =
        (0..ui.playlist_rows.row_count()).filter_map(|i| ui.playlist_rows.row_data(i)).filter(|r| r.checked).collect();
    if chosen.is_empty() {
        d.set_message(t("Check the videos you want first").into());
        return;
    }
    let folder = d.get_folder().trim().to_string();
    if folder.is_empty() {
        d.set_message(t("Choose where to save the videos").into());
        return;
    }
    let folder = PathBuf::from(folder);
    let base_q = QUALITIES[d.get_quality().clamp(0, QUALITIES.len() as i32 - 1) as usize];
    let base_ext = match base_q { "audio" => "m4a", "mp3" => "mp3", _ => "mp4" };
    let (pick_q, ext, sub_template, sub_convert) =
        subs::request_fields(d.get_sub_mode(), d.get_sub_lang().as_str(), base_q, base_ext);
    let title = d.get_playlist_title().to_string();
    let name: String = if title.trim().is_empty() { "Playlist".into() } else { title.trim().chars().take(60).collect() };
    let queue = match dedup::find_existing_queue(&ui.ctx.mgr, &name, &folder) {
        Some(q) => q,
        None => match ui.ctx.mgr.add_queue(&name, d.get_parallel().clamp(1, 3) as u32, Some(folder.clone())) {
            Ok(q) => q,
            Err(e) => {
                d.set_message(format!("{e:#}").into());
                return;
            }
        },
    };
    if let Some(mut def) = ui.ctx.mgr.settings().queue(queue).cloned() {
        def.retries = PLAYLIST_RETRIES;
        def.max_parallel = d.get_parallel().clamp(1, 3) as u32;
        if def.folder.is_none() {
            def.folder = Some(folder.clone());
        }
        let _ = ui.ctx.mgr.update_queue(def);
    }
    let lang_pref = d.get_sub_lang().trim().to_string();
    if d.get_sub_mode() > 0 && !lang_pref.is_empty() {
        let mut s = ui.ctx.mgr.settings();
        if s.subtitle_langs != lang_pref {
            s.subtitle_langs = lang_pref;
            let _ = ui.ctx.mgr.set_settings(s);
        }
    }
    let total_entries = ui.playlist_rows.row_count().max(chosen.len());
    let width = total_entries.to_string().len().max(2);
    let group = Some(ui.ctx.mgr.new_group());
    for r in &chosen {
        let mut file = filename::sanitize(r.name.trim_start_matches('\u{200F}'));
        if d.get_numbered() {
            let n: usize = r.text.parse().unwrap_or(0);
            file = format!("{n:0width$} - {file}");
        }
        ui.ctx.mgr.add(NewDownload {
            url: r.url.to_string(),
            file_name: Some(format!("{file}.{ext}")),
            save_dir: Some(folder.clone()),
            source_page: Some(r.url.to_string()),
            pick_quality: Some(pick_q.clone()),
            subtitle: sub_template.clone(),
            sub_convert: sub_convert.clone(),
            queue: Some(queue),
            description: Some(title.clone()).filter(|t| !t.is_empty()),
            group,
            start: false,
            ..Default::default()
        });
    }
    state.lock_ok().cancel_bg();
    if d.get_start_now() {
        ui.ctx.mgr.start_queue(queue);
    }
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_view(format!("queue:{queue}").into());
    }
    let _ = std::fs::create_dir_all(&folder);
    let _ = d.hide();
    refresh(ui);
}
