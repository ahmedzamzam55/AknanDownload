use super::actions::open_download;
use super::add::early_download;
use super::log::note;
use super::modal::message;
use super::state::Ui;
use crate::i18n::tf;
use crate::{api, preview, sound};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use swiftget_engine::{DownloadKind, Event};

thread_local! {
    // downloads the server refused (403 / 401) handed to the browser:
    // address -> (download, when, times)
    static RELAYED: RefCell<HashMap<String, (u64, Instant, u32)>> = RefCell::new(HashMap::new());
}

/// Sounds and messages for what the download engine reports.
pub(super) fn handle_events(ui: &Ui) {
    let events = ui.ctx.mgr.take_events();
    if events.is_empty() {
        return;
    }
    let s = ui.ctx.mgr.settings();
    let early = early_download(&ui.ctx);
    // a finished file: the preview names of it are not needed any more
    preview::clean_up();
    for ev in events {
        // refused at once (the site lets only the browser in): the browser
        // is asked for it, with the site's sign-in, and gives it back
        if let Event::Failed(id) = ev {
            if relay_refused(ui, id) {
                continue;
            }
        }
        match ev {
            Event::Completed(id) | Event::Failed(id) if Some(id) == early => {}
            Event::Completed(_) if s.sounds_on => sound::play(&s.sound_complete),
            Event::Failed(_) if s.sounds_on => sound::play(&s.sound_failed),
            Event::QueueStarted(_) if s.sounds_on => sound::play(&s.sound_queue_start),
            Event::QueueDone(_) if s.sounds_on => sound::play(&s.sound_queue_done),
            Event::QuotaReached if s.quota_warn => message(&tf(
                "The download limit of {} MB every {} hours was reached, so the downloads were stopped.\nThey continue by themselves when the limit allows it again (Options > Connection).",
                &[&s.quota_mb, &s.quota_hours],
            )),
            Event::OpenFile(id) => {
                if let Some(v) = ui.ctx.mgr.get(id) {
                    open_download(&v.save_path);
                }
            }
            _ => {}
        }
    }
}

/// The server refused a download before its first byte (HTTP 403 / 401):
/// many sites let only the browser in (its sign-in, its exact request).
/// The first time, the address is opened in the browser: the add-in takes
/// that download with everything the browser sent and it comes back to this
/// same download. Refused again: the browser keeps it and downloads it
/// itself. true = handled (no "failed" sound / message).
fn relay_refused(ui: &Ui, id: u64) -> bool {
    let Some(v) = ui.ctx.mgr.get(id) else { return false };
    let err = v.error.clone().unwrap_or_default();
    let refused = err.contains("HTTP 403") || err.contains("HTTP 401");
    // a site's video (yt-dlp found its link): a fresh link is fetched by the
    // engine itself, the browser can't help with it
    if v.kind != DownloadKind::Http || v.downloaded > 0 || !refused || !v.url.starts_with("http") || v.source_key.is_some() {
        return false;
    }
    let times = RELAYED.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|_, (_, at, _)| at.elapsed() < Duration::from_secs(1800));
        let e = r.entry(v.url.clone()).or_insert((id, Instant::now(), 0));
        e.0 = id;
        e.1 = Instant::now();
        e.2 += 1;
        e.2
    });
    if times > 2 {
        return false;
    }
    if times == 2 {
        // refused with the browser's own request too: the browser downloads it
        api::leave_to_browser(&v.url);
        note(&format!("download {id}: refused again, left to the browser"));
    } else {
        note(&format!("download {id}: refused by the server, asking the browser"));
    }
    let _ = open::that_detached(&v.url);
    true
}

/// The add-in sent a download we asked the browser for: it belongs to the
/// refused one, which goes on with the browser's request.
pub(super) fn relayed_download(url: &str) -> Option<u64> {
    RELAYED.with(|r| {
        r.borrow()
            .get(url)
            .filter(|(_, at, times)| *times == 1 && at.elapsed() < Duration::from_secs(300))
            .map(|(id, _, _)| *id)
    })
}
