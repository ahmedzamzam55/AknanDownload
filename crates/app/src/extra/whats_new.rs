use crate::i18n::{t, tf};
use crate::*;

/// What the newest version brings (shown once after an automatic update).
pub(crate) const WHATS_NEW: [&str; 9] = [
    "Subtitles everywhere: download videos with matching subtitles (separate SRT/VTT or burned in) or grab all available subtitle tracks in one click, with automatic retry on temporary site rate limits.",
    "YouTube Kids and faster qualities: Kids videos download smoothly now, and video quality options appear significantly faster across supported sites.",
    "Playlists upgraded: see per-video and total selected sizes, resize the video title column, number tracks oldest-first, and automatically skip videos you already downloaded.",
    "Safer Add Download workflow: edit the final file name right inside Download File Info, resolve existing file conflicts inline (safe replace, auto-number, or rename), and keep pre-downloaded bytes safely in temporary files until you press Start.",
    "Safe replacement & cleaner windows: replacing an existing file keeps the old copy safe until the new one finishes 100%, and blank/transparent window glitches and post-cancel stalls are resolved.",
    "Stronger Site Grabber & cleaner queues: discovers more hidden HTML/CSS/JSON links (plus a browser right-click guide for dynamic JS pages), while queue views show only unfinished items.",
    "Drag & Move: drag completed files or folders straight from Aknan Download into Windows Explorer to move on the same drive or copy across drives just like Explorer.",
    "Fewer antivirus false positives & verified builds: no background Windows CLI tools are spawned during runtime or setup, and every release includes SHA256SUMS.txt checksums.",
    "HLS & .ts video overhaul: handles UTF-8 BOM playlists, spoofed headers before sync bytes, separate audio tracks, fMP4 containers, inline keys, and reliable MP4 remuxing.",
];

thread_local! {
    static WHATS_NEW_WIN: std::cell::RefCell<Option<WhatsNewDialog>> = const { std::cell::RefCell::new(None) };
}

/// After an update: what is new, in its own window (right to left in
/// Arabic), with a word about supporting the free program.
pub(crate) fn whats_new(ui: &Ui) {
    let Ok(d) = WhatsNewDialog::new() else { return };
    crate::set_theme_of(d.global::<Theme>(), &crate::Look::of(&ui.ctx.mgr.settings()));
    d.set_rtl(crate::i18n::is_arabic());
    d.set_heading(tf("Aknan Download was updated to version {} by itself.", &[&env!("CARGO_PKG_VERSION")]).into());
    let items: Vec<SharedString> = WHATS_NEW.iter().map(|l| t(l).into()).collect();
    d.set_items(ModelRc::new(VecModel::from(items)));
    d.on_open_link(|u| {
        if u.starts_with("https://") {
            let _ = open::that_detached(u.as_str());
        }
    });
    let w = d.as_weak();
    d.on_close_window(move || {
        if let Some(d) = w.upgrade() {
            let _ = d.hide();
        }
        // let the window go after this click
        slint::Timer::single_shot(std::time::Duration::from_millis(1), || WHATS_NEW_WIN.with(|x| *x.borrow_mut() = None));
    });
    open_window!(d);
    WHATS_NEW_WIN.with(|x| *x.borrow_mut() = Some(d));
}

/// The first video in AV1 (a much smaller file of the same picture, what
/// Facebook and YouTube give in their best qualities): Windows plays its
/// sound but shows no picture without Microsoft's free AV1 extension. Said
/// once, a moment after it finished.
pub(crate) fn av1_tip(ui: &Ui, done: &[u64]) {
    let s = ui.ctx.mgr.settings();
    if s.av1_tip_shown {
        return;
    }
    let av1 = done
        .iter()
        .filter_map(|id| ui.ctx.mgr.get(*id))
        .any(|v| v.source_key.as_deref().is_some_and(|k| k.contains("AV1")) && v.status == Status::Completed);
    if !av1 {
        return;
    }
    let mut s = s;
    s.av1_tip_shown = true;
    let _ = ui.ctx.mgr.set_settings(s);
    slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
        let text = t("This video is in AV1 (the same picture in a much smaller file). If your player plays only its sound, without the picture, get Microsoft's free \"AV1 Video Extension\" (or play it with VLC).\n\nOpen it in the Microsoft Store now?");
        if crate::ask(&text) {
            let _ = open::that_detached("ms-windows-store://search/?query=AV1%20Video%20Extension");
        }
    });
}

/// `n` downloads just finished well: tracked in settings.
pub(crate) fn finished_well(ui: &Ui, n: usize) {
    if n == 0 {
        return;
    }
    let mut s = ui.ctx.mgr.settings();
    s.finished_count = s.finished_count.saturating_add(n as u64);
    let _ = ui.ctx.mgr.set_settings(s);
}
