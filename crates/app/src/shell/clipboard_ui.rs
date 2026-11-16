use super::add_open::open_add_dialog;
use super::batch::open_batch;
use super::state::{Ui, VideoExtra};
use crate::{clipboard, torrent_ui};
use swiftget_engine::RequestHeaders;

pub(super) fn is_http_url(t: &str) -> Option<String> {
    let t = t.trim();
    is_download_address(t).then(|| t.to_string()).filter(|s| !s.contains(char::is_whitespace))
}

/// An address the program downloads from: http(s), or a file on an FTP server.
pub(crate) fn is_download_address(t: &str) -> bool {
    let l = t.trim_start().to_ascii_lowercase();
    ["http://", "https://", "ftp://", "ftps://"].iter().any(|p| l.starts_with(p))
}

/// Every http(s) link in a block of text, one per line or separated by spaces.
pub(crate) fn extract_links(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|l| l.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>' || c == ','))
        .filter(|l| is_download_address(l) && url::Url::parse(l).is_ok())
        .map(|l| l.to_string())
        .collect()
}

pub(super) fn paste_from_clipboard(ui: &Ui) {
    let raw = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
        .unwrap_or_default();
    let trimmed = raw.trim();
    if !trimmed.is_empty()
        && !trimmed.contains('\n')
        && (trimmed.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("magnet:"))
            || torrent_ui::looks_like_torrent(trimmed, None))
    {
        torrent_ui::open_source(trimmed);
        return;
    }
    let links = extract_links(&raw);
    if links.len() >= 2 {
        open_batch(ui, raw);
    } else if let Some(url) = links.into_iter().next() {
        open_add_dialog(&ui.ctx, url, RequestHeaders::default(), None, VideoExtra::default());
    } else {
        let url = clipboard::downloadable_url(trimmed).or_else(|| is_http_url(trimmed)).unwrap_or_default();
        open_add_dialog(&ui.ctx, url, RequestHeaders::default(), None, VideoExtra::default());
    }
}
