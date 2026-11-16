//! "Download a part of a video": the window where an address, the quality,
//! from / to and the kind of cut are chosen, with a look at the part before
//! it is downloaded. The browser add-in sends parts too (`add_part`).

mod actions;
mod wire;

use crate::i18n::t;
use crate::*;
use std::cell::RefCell;
use swiftget_engine::ytdlp::{self, YtOption};
use swiftget_engine::{clock, Clip};
use tokio_util::sync::CancellationToken;

/// The longest preview (seconds): the first minute of the part.
pub(super) const PREVIEW_MAX: f64 = 60.0;

#[derive(Default)]
pub(super) struct State {
    pub(super) page: String,
    pub(super) title: String,
    pub(super) options: Vec<YtOption>,
    pub(super) preview: Option<CancellationToken>,
}

thread_local! {
    static WIN: RefCell<Option<ClipDialog>> = const { RefCell::new(None) };
    pub(super) static STATE: RefCell<State> = RefCell::new(State::default());
}

pub(super) fn is_web(u: &str) -> bool {
    let u = u.trim();
    u.starts_with("https://") || u.starts_with("http://")
}

/// The name of the part's file: the video's title and the kind of file of
/// the choice (sound alone in a .webm is an Opus sound: .ogg).
pub(crate) fn part_name(title: &str, ext: &str, key: &str) -> String {
    let ext = if key == ytdlp::MP3_KEY {
        "mp3"
    } else if key.starts_with("audio|") && matches!(ext, "webm" | "opus") {
        "ogg"
    } else if key.starts_with("audio|") {
        "m4a"
    } else {
        "mp4"
    };
    let stem = filename::sanitize(title.trim());
    let stem = if stem.is_empty() { "video".to_string() } else { stem };
    format!("{stem}.{ext}")
}

/// A part sent by the browser add-in: straight to the list.
pub(crate) fn add_part(ui: &Ui, req: NewDownload) {
    let id = ui.ctx.mgr.add(req);
    show_progress_for_start(ui, id);
    refresh(ui);
}

/// Open the window (with `url` already looked up, when given).
pub(crate) fn open(ui: &Ui, url: Option<String>) {
    let made = WIN.with(|w| w.borrow().is_some());
    if !made {
        let Ok(d) = ClipDialog::new() else { return };
        wire::wire(ui, &d);
        WIN.with(|w| *w.borrow_mut() = Some(d));
    }
    WIN.with(|w| {
        let w = w.borrow();
        let Some(d) = w.as_ref() else { return };
        crate::set_theme_of(d.global::<Theme>(), &crate::Look::of(&ui.ctx.mgr.settings()));
        let was_open = d.window().is_visible();
        match url.filter(|u| is_web(u)) {
            Some(u) => {
                d.set_url(u.into());
                d.invoke_fetch();
            }
            None if !was_open && d.get_url().is_empty() => {
                // an address in the clipboard is the likely one
                let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
                if is_web(&clip) && !clip.contains(char::is_whitespace) {
                    d.set_url(clip.trim().into());
                }
            }
            None => {}
        }
        open_window!(d);
        d.window().set_minimized(false);
    });
}

fn with_win(f: impl FnOnce(&ClipDialog)) {
    WIN.with(|w| {
        if let Some(d) = w.borrow().as_ref() {
            f(d);
        }
    });
}

/// From / to as they are now, or what is wrong with them.
pub(super) fn chosen(d: &ClipDialog) -> Result<Clip, String> {
    let (a, b) = (d.get_from() as f64, d.get_to() as f64);
    let dur = d.get_duration() as f64;
    if dur > 0.0 && a >= dur {
        return Err(t("The start is after the end of the video."));
    }
    if b <= a {
        return Err(t("The end must come after the start."));
    }
    let b = if dur > 0.0 { b.min(dur) } else { b };
    Clip::new(a, b, d.get_cut() == 1).ok_or_else(|| t("The part must be at least half a second long."))
}

pub(super) fn show_length(d: &ClipDialog) {
    let len = (d.get_to() - d.get_from()).max(0.0) as f64;
    d.set_length_text(if len > 0.0 { clock(len, ':').into() } else { "—".into() });
}

/// The window is closed by the program (it quits).
#[allow(dead_code)]
pub(crate) fn close() {
    with_win(|d| {
        let _ = d.hide();
    });
}

#[cfg(test)]
mod tests {
    use super::part_name;

    #[test]
    fn names_of_parts() {
        assert!(part_name("My: video", "webm", "1080p|WEBM").ends_with(".mp4"));
        assert_eq!(part_name("Song", "m4a", "audio|M4A"), "Song.m4a");
        assert_eq!(part_name("Song", "webm", "audio|WEBM"), "Song.ogg");
        assert_eq!(part_name("Song", "m4a", swiftget_engine::ytdlp::MP3_KEY), "Song.mp3");
        assert!(part_name("  ", "mp4", "720p|MP4").ends_with(".mp4"));
    }
}
