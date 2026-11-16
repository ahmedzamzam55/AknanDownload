use super::Item;
use crate::*;
use std::sync::atomic::Ordering;
use swiftget_engine::grabber;
use swiftget_engine::sync::LockOk;
use tokio_util::sync::CancellationToken;

pub(super) fn item_from(url: &str, text: &str, page: &str) -> Item {
    named_item(url, filename::from_url(url).unwrap_or_else(|| "download".into()), text, page)
}

pub(super) fn named_item(url: &str, name: String, text: &str, page: &str) -> Item {
    let ext = filename::extension(&name);
    let is_page = grabber::is_page_ext(&ext);
    Item {
        checked: !is_page,
        url: url.to_string(),
        kind: if is_page { "text" } else { kind_for(&name) },
        ext: ext.to_uppercase(),
        name,
        text: text.trim().chars().take(200).collect(),
        page: page.to_string(),
        is_page,
        size: None,
        gone: None,
    }
}

/// What the server said about a link: a file (its real name and size), a
/// page, or no good.
pub(super) fn apply_answer(it: &mut Item, c: &grabber::Checked) {
    match c {
        grabber::Checked::File { name, size } => {
            let was_page = it.is_page;
            if !name.is_empty() && name != "download" {
                let t = named_item(&it.url, name.clone(), &it.text, &it.page);
                it.name = t.name;
                it.ext = t.ext;
                it.kind = t.kind;
            }
            it.is_page = false;
            it.size = *size;
            // a link that only looked like a page is a file: wanted
            if was_page {
                it.checked = true;
            }
        }
        grabber::Checked::Page => {
            it.is_page = true;
            it.kind = "text";
            it.checked = false;
        }
        grabber::Checked::Gone(why) => {
            it.gone = Some(why.clone());
            it.checked = false;
        }
    }
}

/// Ask the server about the links (a few at a time, two per site): which
/// are files, their real names and sizes, which are gone.
pub(super) fn ask_about(ui: &Ui, links: Vec<String>, headers: RequestHeaders) {
    if links.is_empty() {
        return;
    }
    let (answers, asking, cancel) = {
        let mut g = ui.grab_state.borrow_mut();
        if let Some(c) = g.asking_cancel.take() {
            c.cancel();
        }
        let c = CancellationToken::new();
        g.asking_cancel = Some(c.clone());
        g.asking.store(links.len(), Ordering::Relaxed);
        (g.answers.clone(), g.asking.clone(), c)
    };
    let client = ui.ctx.mgr.client();
    let ua = ui.ctx.mgr.settings().user_agent.clone();
    ui.ctx.rt.spawn(async move {
        let all = Arc::new(tokio::sync::Semaphore::new(6));
        let mut hosts: std::collections::HashMap<String, Arc<tokio::sync::Semaphore>> = Default::default();
        let mut jobs = tokio::task::JoinSet::new();
        for url in links {
            let host = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            let per = hosts.entry(host).or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(2))).clone();
            let (all, client, headers, ua, answers, asking, cancel) =
                (all.clone(), client.clone(), headers.clone(), ua.clone(), answers.clone(), asking.clone(), cancel.clone());
            jobs.spawn(async move {
                let _a = per.acquire_owned().await;
                let _b = all.acquire_owned().await;
                if !cancel.is_cancelled() {
                    let c = grabber::check_link(&client, &url, &headers, &ua).await;
                    if !cancel.is_cancelled() {
                        answers.lock_ok().push((url, c));
                    }
                }
                asking.fetch_sub(1, Ordering::Relaxed);
            });
        }
        while jobs.join_next().await.is_some() {}
    });
}
