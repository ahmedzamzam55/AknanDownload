use super::activation::cli_download;
use super::add_open::{open_add_dialog, show_progress_for_start};
use super::events::relayed_download;
use super::log::note;
use super::modal::{confirm, modal_open};
use super::rows::refresh;
use super::state::{Ui, VideoExtra};
use super::window::bring_to_front;
use crate::i18n::t;
use crate::{api, browser, clipui, extra, grab, playlist, torrent_ui};
use slint::{Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use swiftget_engine::{filename, NewDownload, RequestHeaders};

thread_local! {
    static LAST_API_ADD: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

pub(super) fn start_api(ui: &Ui, server: Option<tiny_http::Server>, main_server: Option<tiny_http::Server>) {
    if server.is_none() && main_server.is_none() {
        return;
    }
    let cfg_mgr = ui.ctx.mgr.clone();
    // the API thread only sends; the UI thread owns the windows
    let (tx, rx) = std::sync::mpsc::channel::<api::ApiEvent>();
    let rx = Rc::new(rx);
    if let Some(main_server) = main_server {
        let (tx, cfg_mgr) = (tx.clone(), cfg_mgr.clone());
        api::serve(main_server, api::Door::Main, move || api::config_json(&cfg_mgr.settings()), move |event| {
            let _ = tx.send(event);
            let _ = slint::invoke_from_event_loop(|| {});
        });
    }
    if let Some(server) = server {
        api::serve(server, api::Door::Front, move || api::config_json(&cfg_mgr.settings()), move |event| {
            let _ = tx.send(event);
            let _ = slint::invoke_from_event_loop(|| {});
        });
    } else {
        drop((tx, cfg_mgr));
    }
    let ui2 = ui.clone();
    let api_timer = Timer::default();
    api_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        if modal_open() {
            return; // the requests wait until the message box is closed
        }
        while let Ok(ev) = rx.try_recv() {
            handle_api(&ui2, ev);
        }
    });
    std::mem::forget(api_timer);
}

/// A request from the browser extension.
fn handle_api(ui: &Ui, event: api::ApiEvent) {
    match event {
        api::ApiEvent::Show => bring_to_front(&ui.ctx),
        api::ApiEvent::Pair(answer) => {
            bring_to_front(&ui.ctx);
            confirm(
                &ui.ctx,
                &t("Connect a browser?"),
                &t("A browser add-in asks to connect to Aknan Download (to send it the downloads and videos of the browser).\n\nSay yes only if you just installed or updated the Aknan Download add-in in your browser."),
                move || {
                    let _ = answer.send(true);
                },
            );
        }
        api::ApiEvent::OldFirefoxAddon => {
            note("the Firefox add-on is older than the program");
            confirm(
                &ui.ctx,
                &t("Update the Firefox add-on"),
                &t("The Aknan Download add-on in Firefox is an older version than the program, so videos and downloads may not work in Firefox.\n\nClick OK: Firefox opens the new add-on. Then click \"Add\" in the message Firefox shows."),
                {
                    let ui = ui.clone();
                    move || browser::update_firefox_addon(&ui)
                },
            );
        }
        api::ApiEvent::Quit => {
            note("closed for the setup");
            let _ = slint::quit_event_loop();
        }
        api::ApiEvent::OpenTorrent(s) => torrent_ui::open_source(&s),
        api::ApiEvent::Cli(ask) => cli_download(ui, ask),
        api::ApiEvent::Playlist(url) => playlist::open_url(ui, &url),
        api::ApiEvent::Links(l) => grab::open_links(ui, l),
        api::ApiEvent::Add(req) => {
            // the same request again within a couple seconds: ignore duplicate clicks/retries
            let dedup_key = format!(
                "{}|{}|{}|{}",
                req.url,
                req.file_name.as_deref().unwrap_or(""),
                req.sub_convert.as_deref().unwrap_or(""),
                req.subtitle.as_ref().map(|s| format!("{}:{}", s.lang, s.burn)).unwrap_or_default()
            );
            let repeat = LAST_API_ADD.with(|l| {
                let mut l = l.borrow_mut();
                let now = Instant::now();
                let same = l.as_ref().map(|(u, at)| *u == dedup_key && now.duration_since(*at) < Duration::from_secs(3)).unwrap_or(false);
                *l = Some((dedup_key, now));
                same
            });
            if repeat {
                return;
            }
            let mut headers = RequestHeaders {
                referer: req.referrer,
                cookies: req.cookies,
                user_agent: req.user_agent,
                extra: Vec::new(),
                auth: None,
                post: req.post.filter(|p| !p.is_empty() && p.len() <= 1 << 20),
                post_type: req.post_type,
            };
            if let Some(list) = &req.headers {
                swiftget_engine::ytdlp::apply_headers(&mut headers, list);
            }
            // a torrent's magnet link from a page: the torrent window asks first
            if swiftget_engine::torrent::is_magnet(&req.url) {
                torrent_ui::open_source(&req.url);
                return;
            }
            // a .torrent file's link: straight to the torrent window too
            if torrent_ui::looks_like_torrent(&req.url, req.file_name.as_deref()) {
                torrent_ui::fetch_and_open(&ui.ctx, req.url, headers, req.file_name);
                return;
            }
            // a refused download we asked the browser for: it goes on with the
            // browser's own request (headers, sign-in)
            if let Some(id) = relayed_download(&req.url).filter(|id| ui.ctx.mgr.get(*id).is_some()) {
                ui.ctx.mgr.set_url(id, &req.url, Some(headers));
                ui.ctx.mgr.start(id);
                refresh(ui);
                return;
            }
            // "Refresh download address": this is the new link we waited for
            if let Some(id) = ui.refresh_id.get() {
                if req.audio_url.is_none() {
                    extra::refresh_received(ui, id, &req.url, headers);
                    return;
                }
            }
            // a part of a video (the add-in's "Download a part"): straight to the list
            if let Some(c) = req.clip.as_ref().and_then(|c| swiftget_engine::Clip::new(c.start, c.end, c.exact)) {
                let key = req.source_key.clone().unwrap_or_default();
                let name = req.file_name.clone().unwrap_or_else(|| "video.mp4".into());
                let (stem, ext) = filename::split_ext(&name);
                clipui::add_part(
                    ui,
                    NewDownload {
                        url: req.url,
                        file_name: Some(clipui::part_name(stem, &ext.to_ascii_lowercase(), &key)),
                        headers,
                        audio_url: req.audio_url,
                        source_page: req.source_page,
                        source_key: req.source_key,
                        clip: Some(c),
                        start: true,
                        ..Default::default()
                    },
                );
                return;
            }
            let extra = VideoExtra {
                audio_url: req.audio_url,
                source_page: req.source_page,
                source_key: req.source_key,
            };
            let s = ui.ctx.mgr.settings();
            if req.direct || (!s.show_start_dialog && ui.ctx.mgr.find_url(&req.url).is_none()) {
                // straight to the list, without hiding behind the "Download File Info" window
                let is_sub_file = req.sub_convert.is_some();
                let id = ui.ctx.mgr.add(NewDownload {
                    url: req.url,
                    file_name: req.file_name,
                    headers,
                    audio_url: extra.audio_url,
                    source_page: extra.source_page,
                    source_key: extra.source_key,
                    group: req.group,
                    subtitle: req.subtitle,
                    sub_convert: req.sub_convert,
                    sub_body: req.sub_body,
                    start: true,
                    ..Default::default()
                });
                if !is_sub_file {
                    show_progress_for_start(ui, id);
                }
                refresh(ui);
                return;
            }
            open_add_dialog(&ui.ctx, req.url, headers, req.file_name, extra);
        }
    }
}
