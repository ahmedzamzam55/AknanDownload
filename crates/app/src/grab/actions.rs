use super::list::show;
use crate::i18n::{t, tf};
use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};
use swiftget_engine::grabber::{self, GrabOptions, GrabProgress};
use swiftget_engine::sync::LockOk;
use tokio_util::sync::CancellationToken;

pub(super) fn explore(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    let start = d.get_start_url().trim().to_string();
    if !(start.starts_with("http://") || start.starts_with("https://")) || url::Url::parse(&start).is_err() {
        d.set_message(t("Type the address of the start page (http:// or https://)").into());
        return;
    }
    d.set_message(SharedString::new());
    let exts: Vec<String> = if d.get_template() == 6 {
        d.get_exts().split_whitespace().map(|e| e.trim_start_matches('.').to_lowercase()).collect()
    } else {
        grabber::template_exts(d.get_template())
    };
    let s = ui.ctx.mgr.settings();
    let opts = GrabOptions {
        start_url: start.clone(),
        depth: d.get_depth().clamp(0, 10) as u32,
        other_sites: d.get_other_sites(),
        no_parent: d.get_no_parent(),
        exts,
        include: Vec::new(),
        exclude: Vec::new(),
        max_pages: 2000,
        parallel: d.get_parallel().clamp(1, 10) as usize,
        headers: {
            // the site the browser handed over: its cookies, so pages
            // behind a sign-in show their files
            let from_browser = ui.grab_state.borrow().headers.clone();
            let host = |u: &str| url::Url::parse(u).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase));
            let same = from_browser.referer.as_deref().and_then(host).is_some_and(|h| Some(h) == host(&start));
            RequestHeaders {
                auth: s.login_for(&start),
                cookies: if same { from_browser.cookies.clone() } else { None },
                user_agent: if same { from_browser.user_agent.clone() } else { None },
                ..Default::default()
            }
        },
        user_agent: s.user_agent.clone(),
    };
    let progress = Arc::new(GrabProgress::default());
    let cancel = CancellationToken::new();
    let finished = Arc::new(AtomicBool::new(false));
    let incoming = Arc::new(Mutex::new(Vec::new()));
    {
        let mut g = ui.grab_state.borrow_mut();
        if let Some(c) = g.cancel.take() {
            c.cancel();
        }
        g.all.clear();
        g.progress = Some(progress.clone());
        g.cancel = Some(cancel.clone());
        g.finished = finished.clone();
        g.incoming = incoming.clone();
        g.exploring = true;
    }
    show(ui);
    d.set_exploring(true);
    d.set_progress_text(t("Starting…").into());
    let client = ui.ctx.mgr.client();
    ui.ctx.rt.spawn(async move {
        let sink = incoming.clone();
        grabber::crawl(client, opts, progress, cancel, move |f| sink.lock_ok().push(f)).await;
        finished.store(true, Ordering::Relaxed);
    });
}

pub(super) fn download_checked(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    let (items, headers) = {
        let g = ui.grab_state.borrow();
        (g.all.iter().filter(|i| i.checked).cloned().collect::<Vec<_>>(), g.headers.clone())
    };
    if items.is_empty() {
        d.set_message(t("Check the files you want first").into());
        return;
    }
    let s = ui.ctx.mgr.settings();
    let queue = s.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id).unwrap_or(MAIN_QUEUE);
    let folder = d.get_folder().trim().to_string();
    let use_text = d.get_use_text();
    let mut added = 0;
    let group = Some(ui.ctx.mgr.new_group());
    for it in items {
        if ui.ctx.mgr.find_url(&it.url).is_some() {
            continue;
        }
        let mut h = headers.clone();
        if h.referer.is_none() && !it.page.is_empty() {
            h.referer = Some(it.page.clone());
        }
        ui.ctx.mgr.add(NewDownload {
            url: it.url.clone(),
            save_dir: (!folder.is_empty()).then(|| PathBuf::from(&folder)),
            headers: h,
            queue: Some(queue),
            description: (use_text && !it.text.is_empty()).then(|| it.text.clone()),
            group,
            start: false,
            ..Default::default()
        });
        added += 1;
    }
    if d.get_start_now() {
        ui.ctx.mgr.start_queue(queue);
    }
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_view(format!("queue:{queue}").into());
    }
    d.set_message(SharedString::new());
    d.set_checked_text(tf("{} file(s) added to the queue", &[&added]).into());
    refresh(ui);
}
