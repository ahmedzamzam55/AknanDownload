//! Site Grabber and "Download all links" (both use the same window).

mod actions;
mod check;
mod list;
mod wire;

pub(crate) use wire::wire;

use crate::i18n::{t, tf};
use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};
use swiftget_engine::grabber::{self, FoundFile, GrabProgress};
use swiftget_engine::sync::LockOk;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct Item {
    pub(super) checked: bool,
    pub(super) url: String,
    pub(super) name: String,
    pub(super) ext: String,
    pub(super) kind: &'static str,
    pub(super) text: String,
    pub(super) page: String,
    pub(super) is_page: bool,
    /// the server's size, once asked
    pub(super) size: Option<u64>,
    /// the server said it is no good (not found...): why
    pub(super) gone: Option<String>,
}

#[derive(Default)]
pub(crate) struct GrabUi {
    pub(super) all: Vec<Item>,
    /// rows on screen -> index in `all`
    pub(super) shown: Vec<usize>,
    pub(super) model: Option<Rc<VecModel<LinkRow>>>,
    pub(super) incoming: Arc<Mutex<Vec<FoundFile>>>,
    pub(super) progress: Option<Arc<GrabProgress>>,
    pub(super) cancel: Option<CancellationToken>,
    pub(super) finished: Arc<AtomicBool>,
    pub(super) exploring: bool,
    /// links mode: the page and its cookies
    pub(super) headers: RequestHeaders,
    /// answers of the server about links (address, what it is)
    pub(super) answers: Arc<Mutex<Vec<(String, grabber::Checked)>>>,
    /// links still being asked about
    pub(super) asking: Arc<std::sync::atomic::AtomicUsize>,
    pub(super) asking_cancel: Option<CancellationToken>,
}

/// Tasks > Run site grabber.
pub(crate) fn open_grabber(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    if d.window().is_visible() && !d.get_links_mode() {
        open_window!(d);
        return;
    }
    list::reset(ui, &d, false);
    list::show(ui);
    open_window!(d);
}

/// "Download all links with Aknan Download" from the browser.
pub(crate) fn open_links(ui: &Ui, l: api::LinksRequest) {
    let Some(d) = ui.grabber.upgrade() else { return };
    list::reset(ui, &d, true);
    {
        let mut g = ui.grab_state.borrow_mut();
        g.headers = RequestHeaders {
            referer: (!l.page.is_empty()).then(|| l.page.clone()),
            cookies: l.cookies.clone(),
            user_agent: l.user_agent.clone(),
            ..Default::default()
        };
        let mut seen = std::collections::HashSet::new();
        for link in &l.links {
            if seen.insert(link.url.clone()) {
                g.all.push(check::item_from(&link.url, &link.text, &l.page));
            }
        }
    }
    d.set_start_url(l.page.clone().into());
    // the files: their real names and sizes; the links that look like pages
    // but may be downloads ("get.php?id=5"): whether they are files
    let (ask, headers) = {
        let g = ui.grab_state.borrow();
        let ask: Vec<String> = g
            .all
            .iter()
            .filter(|i| !i.is_page || grabber::looks_like_download(&i.url, &i.text))
            .map(|i| i.url.clone())
            .take(400)
            .collect();
        (ask, g.headers.clone())
    };
    check::ask_about(ui, ask, headers);
    list::show(ui);
    bring_to_front(&ui.ctx);
    open_window!(d);
}

/// New files from the explorer, and its counters (10 times a second).
pub(crate) fn poll(ui: &Ui) {
    let answers: Vec<(String, grabber::Checked)> = std::mem::take(&mut *ui.grab_state.borrow().answers.lock_ok());
    if !answers.is_empty() {
        {
            let mut g = ui.grab_state.borrow_mut();
            let at: std::collections::HashMap<String, usize> = g.all.iter().enumerate().map(|(i, it)| (it.url.clone(), i)).collect();
            for (url, c) in &answers {
                if let Some(&i) = at.get(url) {
                    check::apply_answer(&mut g.all[i], c);
                }
            }
        }
        list::show(ui);
    }
    if let Some(d) = ui.grabber.upgrade() {
        let left = ui.grab_state.borrow().asking.load(Ordering::Relaxed);
        if d.get_links_mode() {
            d.set_progress_text(if left > 0 { tf("Asking the server about the links: {} left", &[&left]).into() } else { SharedString::new() });
        }
    }
    let (new, progress, finished) = {
        let g = ui.grab_state.borrow();
        if !g.exploring {
            return;
        }
        let new: Vec<FoundFile> = std::mem::take(&mut *g.incoming.lock_ok());
        (new, g.progress.clone(), g.finished.load(Ordering::Relaxed))
    };
    let Some(d) = ui.grabber.upgrade() else { return };
    if !new.is_empty() {
        let mut g = ui.grab_state.borrow_mut();
        for f in new {
            let name = if f.name.is_empty() { filename::from_url(&f.url).unwrap_or_else(|| "download".into()) } else { f.name.clone() };
            g.all.push(check::named_item(&f.url, name, &f.text, &f.page));
        }
        drop(g);
        list::show(ui);
    }
    if let Some(p) = progress {
        let current = p.current.lock().map(|c| c.clone()).unwrap_or_default();
        let text = tf(
            "{} of {} pages explored  ·  {} files found",
            &[&p.pages_done.load(Ordering::Relaxed), &p.pages_queued.load(Ordering::Relaxed), &p.files.load(Ordering::Relaxed)],
        ) + &if finished { t("  ·  done") } else { format!("  ·  {current}") };
        d.set_progress_text(text.into());
    }
    if finished {
        ui.grab_state.borrow_mut().exploring = false;
        d.set_exploring(false);
    }
}
