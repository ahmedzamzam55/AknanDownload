//! "Add a torrent": a magnet link or a .torrent file, the files it shares
//! (each can be left out), the folder, and off it goes like any download.

mod wire;

pub(crate) use wire::wire;

use crate::i18n::tf;
use crate::*;
use swiftget_engine::torrent;

thread_local! {
    /// opens the window from places that only have the shared context
    pub(super) static OPENER: std::cell::RefCell<Option<Ui>> = const { std::cell::RefCell::new(None) };
}

/// Open the torrent window with this magnet link / .torrent file.
pub(crate) fn open_source(source: &str) {
    if let Some(ui) = OPENER.with(|o| o.borrow().clone()) {
        open(&ui, Some(source));
    }
}

/// A web address of a .torrent file (by its name), not something to save as it is.
pub(crate) fn looks_like_torrent(url: &str, file_name: Option<&str>) -> bool {
    let path = url::Url::parse(url).map(|u| u.path().to_ascii_lowercase()).unwrap_or_default();
    (url.starts_with("http://") || url.starts_with("https://"))
        && (path.ends_with(".torrent") || file_name.is_some_and(|n| n.trim().to_ascii_lowercase().ends_with(".torrent")))
}

/// A .torrent link clicked in the browser: the file is fetched at once and
/// the torrent window shows what it shares (no extra step). If it can't be
/// read as a torrent, it goes the ordinary way ("Download File Info").
pub(crate) fn fetch_and_open(ctx: &Ctx, url: String, headers: RequestHeaders, file_name: Option<String>) {
    let mgr = ctx.mgr.clone();
    ctx.rt.spawn(async move {
        let result = mgr.fetch_torrent_file(&url, &headers).await;
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok(path) => open_source(&path.display().to_string()),
            Err(_) => {
                if let Some(ui) = OPENER.with(|o| o.borrow().clone()) {
                    open_add_dialog(&ui.ctx, url, headers, file_name, VideoExtra::default());
                }
            }
        });
    });
}

pub(crate) fn open(ui: &Ui, source: Option<&str>) {
    let Some(d) = ui.torrent.upgrade() else { return };
    match source {
        Some(s) => d.set_source(s.into()),
        None if !d.window().is_visible() => {
            // a magnet link in the clipboard is the likely one
            let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
            d.set_source(if torrent::is_magnet(&clip) { clip.trim().into() } else { SharedString::new() });
        }
        None => {}
    }
    d.invoke_load();
    open_window!(d);
    d.window().set_minimized(false);
}

pub(super) fn checked_text(rows: &VecModel<LinkRow>) -> String {
    let n = (0..rows.row_count()).filter_map(|i| rows.row_data(i)).filter(|r| r.checked).count();
    tf("{} of {} files checked", &[&n, &rows.row_count()])
}

/// "1,3,4": the torrent's numbers of the checked files (kept in each row's
/// `text`), `None` when all are checked.
pub(super) fn selection(rows: &VecModel<LinkRow>) -> Option<String> {
    let all: Vec<LinkRow> = (0..rows.row_count()).filter_map(|i| rows.row_data(i)).collect();
    if all.iter().all(|r| r.checked) {
        return None;
    }
    Some(all.iter().filter(|r| r.checked).map(|r| r.text.to_string()).collect::<Vec<_>>().join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(checked: bool, index: usize) -> LinkRow {
        LinkRow { checked, text: index.to_string().into(), ..Default::default() }
    }

    #[test]
    fn chosen_files() {
        // the torrent's own numbers (a padding file was number 2)
        let rows = VecModel::from(vec![row(true, 1), row(false, 3), row(true, 4)]);
        assert_eq!(selection(&rows).as_deref(), Some("1,4"));
        let all = VecModel::from(vec![row(true, 1), row(true, 2)]);
        assert_eq!(selection(&all), None);
        let none = VecModel::from(vec![row(false, 1)]);
        assert_eq!(selection(&none).as_deref(), Some(""));
    }
}
