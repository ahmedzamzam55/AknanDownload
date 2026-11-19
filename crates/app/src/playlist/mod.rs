//! "Download a playlist": every video of a playlist in one go, in
//! the quality you choose, with sizes, oldest-first numbering, duplicate skip,
//! and flexible subtitle modes.

mod controls;
mod dedup;
mod subs;

use controls::wire_controls;

use crate::i18n::{t, tf};
use crate::*;
use std::collections::HashMap;
use swiftget_engine::sync::LockOk;
use swiftget_engine::ytdlp::{self, PlaylistEntry, YtOption};
use tokio_util::sync::CancellationToken;

const PLAYLIST_RETRIES: u32 = 3;
const QUALITIES: [&str; 11] = ["best", "2160", "1440", "1080", "720", "480", "360", "240", "144", "audio", "mp3"];

#[derive(Default)]
struct PlaylistState {
    entries: Vec<PlaylistEntry>,
    exact: HashMap<usize, Vec<YtOption>>,
    done: Vec<bool>,
    sizes_cancel: Option<CancellationToken>,
}

impl PlaylistState {
    fn cancel_bg(&mut self) {
        if let Some(c) = self.sizes_cancel.take() {
            c.cancel();
        }
    }
}

pub(crate) fn open(ui: &Ui) {
    let Some(d) = ui.playlist.upgrade() else { return };
    if !d.window().is_visible() {
        let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
        let clip = clip.trim();
        if clip.contains("list=") || clip.contains("/playlist") || clip.contains("/@") {
            d.set_url(clip.into());
        }
        d.set_message(SharedString::new());
        subs::apply_defaults(&d, &ui.ctx.mgr.settings());
    }
    open_window!(d);
    d.window().set_minimized(false);
}

pub(crate) fn open_url(ui: &Ui, url: &str) {
    let Some(d) = ui.playlist.upgrade() else { return };
    d.set_url(url.into());
    d.set_message(SharedString::new());
    subs::apply_defaults(&d, &ui.ctx.mgr.settings());
    open_window!(d);
    d.window().set_minimized(false);
    if !d.get_busy() {
        d.invoke_fetch();
    }
}

fn fmt_len(secs: u64) -> String {
    if secs == 0 {
        String::new()
    } else if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

fn selected_quality(d: &PlaylistDialog) -> &'static str {
    QUALITIES[d.get_quality().clamp(0, QUALITIES.len() as i32 - 1) as usize]
}

fn row_size(st: &PlaylistState, idx: usize, quality: &str, sub_mode: i32) -> (Option<u64>, bool) {
    if sub_mode >= 4 {
        return (None, true);
    }
    if let Some(b) = st.exact.get(&idx).and_then(|o| ytdlp::pick_option(o, quality)).map(|o| o.size).filter(|&s| s > 0) {
        return (Some(b), true);
    }
    if let Some(b) = st.entries.get(idx).and_then(|e| e.size_bytes).filter(|&s| s > 0) {
        return (Some(b), true);
    }
    (st.entries.get(idx).and_then(|e| ytdlp::estimate_size(e.duration, quality)), false)
}

fn fmt_row_size(bytes: Option<u64>, exact: bool) -> String {
    match bytes {
        Some(b) if exact => fmt_size(b),
        Some(b) => format!("~{}", fmt_size(b)),
        None => String::new(),
    }
}

fn update_checked_text(rows: &VecModel<LinkRow>, d: &PlaylistDialog, st: &PlaylistState) {
    let (q, sm) = (selected_quality(d), d.get_sub_mode());
    let (mut count, mut sum, mut all_exact) = (0usize, 0u64, true);
    for i in 0..rows.row_count() {
        if let Some(r) = rows.row_data(i) {
            if r.checked {
                count += 1;
                if let (Some(b), exact) = row_size(st, i, q, sm) {
                    sum = sum.saturating_add(b);
                    if !exact {
                        all_exact = false;
                    }
                }
            }
        }
    }
    let base = tf("{} of {} videos checked", &[&count, &rows.row_count()]);
    let label = if sum > 0 {
        let total = if all_exact { fmt_size(sum) } else { format!("~{}", fmt_size(sum)) };
        format!("{base} · {total}")
    } else {
        base
    };
    d.set_checked_text(label.into());
}

fn sync_rows_meta(rows: &VecModel<LinkRow>, d: &PlaylistDialog, st: &PlaylistState) {
    let (q, sm, oldest, total) = (selected_quality(d), d.get_sub_mode(), d.get_oldest_first(), rows.row_count());
    for i in 0..total {
        if let Some(mut r) = rows.row_data(i) {
            let (sz, exact) = row_size(st, i, q, sm);
            r.text = dedup::display_number(i, total, oldest).to_string().into();
            r.size = fmt_row_size(sz, exact).into();
            rows.set_row_data(i, r);
        }
    }
    update_checked_text(rows, d, st);
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.playlist.upgrade() else { return };
    let rows = ui.playlist_rows.clone();
    let state = Arc::new(Mutex::new(PlaylistState::default()));
    d.set_entries(ModelRc::from(rows.clone()));
    subs::apply_defaults(&d, &ui.ctx.mgr.settings());
    {
        let (w, mgr, st2) = (d.as_weak(), ui.ctx.mgr.clone(), state.clone());
        d.on_fetch(move || {
            let Some(d) = w.upgrade() else { return };
            let url = d.get_url().trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                d.set_message(t("Paste the playlist address (https://…/playlist?list=…)").into());
                return;
            }
            st2.lock_ok().cancel_bg();
            d.set_message(SharedString::new());
            d.set_busy(true);
            d.set_status(t("Getting the list of videos… (a few seconds)").into());
            let (w2, base, lang, st3, mgr2) =
                (d.as_weak(), mgr.settings().dir_for("Video"), crate::i18n::effective(&mgr.settings().language).to_string(), st2.clone(), mgr.clone());
            std::thread::spawn(move || {
                let result = ytdlp::playlist(&url, &lang);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(d) = w2.upgrade() else { return };
                    d.set_busy(false);
                    match result {
                        Ok(p) if p.entries.is_empty() => d.set_status(t("No videos were found at this address.").into()),
                        Ok(p) => apply_fetched(&d, &mgr2, &st3, &base, p),
                        Err(e) => d.set_status(tf("Could not get the list: {}", &[&crate::err_text(&format!("{e:#}"))]).into()),
                    }
                });
            });
        });
    }
    wire_controls(&d, &rows, &state, ui);
}

fn apply_fetched(d: &PlaylistDialog, mgr: &Arc<Manager>, state: &Arc<Mutex<PlaylistState>>, base: &Path, p: ytdlp::PlaylistInfo) {
    let model = d.get_entries();
    let Some(rows) = model.as_any().downcast_ref::<VecModel<LinkRow>>() else { return };
    let folder = base.join(filename::sanitize(&p.title));
    let keys = dedup::completed_keys(mgr);
    let stems = dedup::disk_stems(&folder);
    let (q, sm, oldest, total) = (selected_quality(d), d.get_sub_mode(), d.get_oldest_first(), p.entries.len());
    let mut done_flags = Vec::with_capacity(total);
    let mut list = Vec::with_capacity(total);
    let mut targets = Vec::new();
    {
        let mut st = state.lock_ok();
        st.cancel_bg();
        st.exact.clear();
        st.entries = p.entries;
        for (i, e) in st.entries.iter().enumerate() {
            let done = dedup::is_already_downloaded(&keys, &stems, &e.url, &e.title);
            done_flags.push(done);
            let bytes = e.size_bytes.or_else(|| ytdlp::estimate_size(e.duration, q));
            let exact = e.size_bytes.is_some();
            if !exact && sm < 4 {
                targets.push((i, e.url.clone()));
            }
            list.push(LinkRow {
                checked: !done,
                name: crate::i18n::shown_name(&e.title).into(),
                rtl: crate::i18n::rtl_text(&e.title),
                kind: "video".into(),
                ext: fmt_len(e.duration).into(),
                size: if sm >= 4 { SharedString::new() } else { fmt_row_size(bytes, exact).into() },
                done,
                text: dedup::display_number(i, total, oldest).to_string().into(),
                url: e.url.clone().into(),
            });
        }
        st.done = done_flags;
    }
    rows.set_vec(list);
    update_checked_text(rows, d, &state.lock_ok());
    d.set_playlist_title(p.title.clone().into());
    d.set_status(tf("{} videos", &[&total]).into());
    d.set_folder(folder.display().to_string().into());
    let cancel = CancellationToken::new();
    state.lock_ok().sizes_cancel = Some(cancel.clone());
    let (w, st2) = (d.as_weak(), state.clone());
    ytdlp::fetch_playlist_sizes(targets, cancel, move |idx, opts| {
        let (w2, st3) = (w.clone(), st2.clone());
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = w2.upgrade() else { return };
            st3.lock_ok().exact.insert(idx, opts);
            let model = d.get_entries();
            if let Some(m) = model.as_any().downcast_ref::<VecModel<LinkRow>>() {
                if let Some(mut r) = m.row_data(idx) {
                    let st = st3.lock_ok();
                    let (sz, exact) = row_size(&st, idx, selected_quality(&d), d.get_sub_mode());
                    r.size = fmt_row_size(sz, exact).into();
                    m.set_row_data(idx, r);
                    update_checked_text(m, &d, &st);
                }
            }
        });
    });
}
