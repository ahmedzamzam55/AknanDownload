//! Site Grabber: explores a web site from a start page and collects the
//! files it links to (images, videos, music, archives, documents...).

use crate::filename;
use crate::model::RequestHeaders;
use reqwest::Client;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

mod fetch;
mod html;
#[cfg(test)]
mod tests;

pub use fetch::{check_link, Checked};
pub use html::{extract_links, is_page_ext, looks_like_download, template_exts};
use fetch::{fetch_page, Fetched};
use html::{allowed, ext_of, wanted_ext};
#[derive(Debug, Clone)]
pub struct GrabOptions {
    pub start_url: String,
    /// levels of pages to explore below the start page (0 = start page only)
    pub depth: u32,
    /// explore pages of other sites too (one level)
    pub other_sites: bool,
    /// don't go above the start page's folder
    pub no_parent: bool,
    /// file types to collect; empty = every file that isn't a web page
    pub exts: Vec<String>,
    /// only addresses matching one of these (* allowed); empty = all
    pub include: Vec<String>,
    /// skip addresses matching any of these
    pub exclude: Vec<String>,
    /// stop after this many pages
    pub max_pages: usize,
    /// pages explored at the same time (1-10)
    pub parallel: usize,
    pub headers: RequestHeaders,
    pub user_agent: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundFile {
    pub url: String,
    pub name: String,
    pub ext: String,
    /// the link's text (used as the description)
    pub text: String,
    /// page the link was found on
    pub page: String,
}

/// Live counters for the grabber window.
#[derive(Default)]
pub struct GrabProgress {
    pub pages_done: AtomicUsize,
    pub pages_queued: AtomicUsize,
    pub files: AtomicUsize,
    pub current: Mutex<String>,
}

/// Links beyond the levels to explore that are asked whether they are files.
const MAX_ASKED: usize = 300;

/// Explore the site and return the files found (in the order found).
pub async fn crawl(
    client: Client,
    opts: GrabOptions,
    progress: Arc<GrabProgress>,
    cancel: CancellationToken,
    on_file: impl Fn(FoundFile) + Send + Sync + 'static,
) -> Vec<FoundFile> {
    let Ok(start) = url::Url::parse(&opts.start_url) else { return Vec::new() };
    let start_host = start.host_str().unwrap_or("").to_ascii_lowercase();
    let start_dir = {
        let p = start.path();
        p[..p.rfind('/').map(|i| i + 1).unwrap_or(p.len())].to_string()
    };
    let on_file = Arc::new(on_file);
    let mut found: Vec<FoundFile> = Vec::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut seen_pages: HashSet<String> = HashSet::new();
    // (url, level, on another site, only to see whether it is a file)
    let mut queue: VecDeque<(String, u32, bool, bool)> = VecDeque::new();
    queue.push_back((opts.start_url.clone(), 0, false, false));
    let mut asked = 0usize;
    seen_pages.insert(opts.start_url.clone());
    progress.pages_queued.store(1, Ordering::Relaxed);
    let parallel = opts.parallel.clamp(1, 10);

    while !queue.is_empty() && !cancel.is_cancelled() {
        let mut batch = Vec::new();
        while batch.len() < parallel {
            match queue.pop_front() {
                Some(item) => batch.push(item),
                None => break,
            }
        }
        let mut tasks = tokio::task::JoinSet::new();
        for (url, level, foreign, only_ask) in batch {
            let client = client.clone();
            let opts2 = opts.clone();
            if let Ok(mut c) = progress.current.lock() {
                *c = url.clone();
            }
            tasks.spawn(async move {
                let page = fetch_page(&client, &url, &opts2).await;
                (url, level, foreign, only_ask, page)
            });
        }
        while let Some(res) = tokio::select! {
            r = tasks.join_next() => r,
            _ = cancel.cancelled() => None,
        } {
            let Ok((url, level, foreign, only_ask, page)) = res else { continue };
            progress.pages_done.fetch_add(1, Ordering::Relaxed);
            let (page_url, html) = match page {
                Some(Fetched::Page(u, h)) if !only_ask => (u, h),
                Some(Fetched::File { url: file_url, name }) => {
                    let ext = filename::extension(&name);
                    let new = seen_files.insert(url.clone()) | seen_files.insert(file_url.clone());
                    if wanted_ext(&opts, &ext) && new {
                        let f = FoundFile { url: url.clone(), name, ext, text: String::new(), page: url.clone() };
                        progress.files.fetch_add(1, Ordering::Relaxed);
                        on_file(f.clone());
                        found.push(f);
                    }
                    continue;
                }
                _ => continue,
            };
            for (link, text) in extract_links(&html, &page_url) {
                let Ok(u) = url::Url::parse(&link) else { continue };
                let host = u.host_str().unwrap_or("").to_ascii_lowercase();
                let ext = ext_of(&u);
                if !allowed(&opts, &link) {
                    continue;
                }
                if is_page_ext(&ext) && (opts.exts.is_empty() || !wanted_ext(&opts, &ext)) {
                    // a page: explore it later if it's within the limits
                    let same = host == start_host;
                    let next_level = level + 1;
                    let ok_site = same || (opts.other_sites && !foreign);
                    let ok_parent = !opts.no_parent || !same || u.path().starts_with(&start_dir);
                    if next_level <= opts.depth
                        && ok_site
                        && ok_parent
                        && seen_pages.len() < opts.max_pages.max(1)
                        && seen_pages.insert(link.clone())
                    {
                        queue.push_back((link, next_level, !same, false));
                        progress.pages_queued.fetch_add(1, Ordering::Relaxed);
                    } else if asked < MAX_ASKED && looks_like_download(&link, &text) && seen_pages.insert(link.clone()) {
                        // too deep to explore, but it may be a file: only asked
                        asked += 1;
                        queue.push_back((link, next_level, !same, true));
                        progress.pages_queued.fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                }
                if wanted_ext(&opts, &ext) && seen_files.insert(link.clone()) {
                    let name = filename::from_url(&link).unwrap_or_else(|| "download".into());
                    let f = FoundFile { url: link, name, ext, text: text.clone(), page: url.clone() };
                    progress.files.fetch_add(1, Ordering::Relaxed);
                    on_file(f.clone());
                    found.push(f);
                }
            }
        }
    }
    found
}
