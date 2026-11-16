use crate::i18n::{t, tf};
use crate::*;

/// What the newest version brings (shown once after an automatic update).
pub(crate) const WHATS_NEW: [&str; 19] = [
    "The Site Grabber finds the files behind download links that show no file name and asks the server for their real names and sizes, a playlist shows one \"Download complete\" window at the end instead of one for every file, and the computer is never shut down or put to sleep while a file is still being put together.",
    "Torrents are shared by the program itself now, with no helper program: they start sooner, use less of the computer, and a stopped torrent goes on after a restart without checking all its pieces again.",
    "FTP and FTPS links download like web links: in parts, with resume, and with the user name and password kept apart from the address.",
    "Checksum: paste an MD5, SHA-1 or SHA-256 value in Download File Info (or the site gives one) and the file is checked when it finishes.",
    "Torrents have their own tab in Properties: the files (choose them while it runs), the people, the trackers and the pieces. Add a tracker, check the files again, copy the magnet link, first and last pieces first, and stop sharing at a ratio you choose.",
    "Safer resume: a file that changed on the server is noticed instead of being joined with the old parts, links that lead through a few pages to the file are followed, and downloads that need a form sent from the browser work.",
    "The speed limit can work only while you use the computer, and a scheduled queue can wake the computer from sleep to start.",
    "Keyboard shortcuts for the list (F1 shows them), a command line for other programs and scripts, and Help > Add Aknan Download to my browser > Test the link checks the add-in step by step.",
    "The add-in works in private windows too (allow it there in the browser), keeps the browser's own download when the program is closed, and its video button can be moved or hidden on a site.",
    "Four new 3D looks (View): 3D, Aurora glass, Soft clay and Cyber neon, with raised buttons, colored pictures, downloads on floating cards and a new download window with a progress ring and the speed.",
    "The setup asks for the language of the program at the first install, and can let torrents through the Windows firewall.",
    "Options > Torrents: make Aknan Download the main program for torrents, or leave magnet links and .torrent files to your own torrent program.",
    "A torrent that is connected but gets nothing looks for other people by itself, and says why it waits.",
    "Playlists: names in the program's language, right-to-left names shown from the right, a few videos at a time, and a video that fails is tried again by itself.",
    "The add-in has one tab for the whole video and one for a part of it, and it never downloads the video that played before in the same player.",
    "The right-click menu of the list is a real Windows menu that is never cut off, in the look of the program.",
    "Sites that refuse extra connections: the download goes on with fewer. Up to 64 connections. A site that asks for a robot check says so, and the Firefox add-on is fetched from the official page when it is missing.",
    "The add-in talks to the program directly, without a port: no more \"Aknan Download is not running\" while it runs, and a closed program is opened for a download.",
    "A power cut no longer loses the download list, the end of a download is never held by one slow connection, and a file that is already in the folder asks whether to replace it.",
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
