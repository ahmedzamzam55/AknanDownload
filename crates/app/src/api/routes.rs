use super::probe::can_fetch;
use super::server::{is_http, reply};
use super::state::{bundled_extension_version, take_for_browser, torrent_source_ok, ui_hangs, REPAIR};
use super::types::{AddRequest, ApiEvent, BrowserQuery, LinksRequest, ResolveRequest};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use tiny_http::{Method, Request};

/// Video look-ups running now (each one is a yt-dlp process).
static RESOLVING: AtomicUsize = AtomicUsize::new(0);
const MAX_RESOLVING: usize = 3;

fn read_body(req: &mut Request, max: u64) -> Option<String> {
    let mut body = String::new();
    req.as_reader().take(max).read_to_string(&mut body).ok().map(|_| body)
}

pub(super) fn dispatch(mut req: Request, method: Method, path: &str, config: &impl Fn() -> String, on_event: &impl Fn(ApiEvent)) {
    match (method, path) {
        (Method::Get, "/ping") => {
            // "ext": the add-in version that came with this program; an older
            // add-in loaded from the program's folder reloads itself
            // "repair": "Repair the browser add-in" was used (the add-in
            // starts again on a new number)
            let body = format!(
                r#"{{"app":"AknanDownload","version":"{}","ext":"{}","repair":{}}}"#,
                env!("CARGO_PKG_VERSION"),
                bundled_extension_version(),
                REPAIR.load(Ordering::Relaxed)
            );
            reply(req, 200, &body)
        }
        (Method::Get, "/config") => reply(req, 200, &config()),
        (Method::Post, "/links") => {
            let Some(body) = read_body(&mut req, 16 << 20) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            match serde_json::from_str::<LinksRequest>(&body) {
                Ok(mut l) => {
                    l.links.retain(|x| is_http(&x.url));
                    on_event(ApiEvent::Links(l));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                Err(_) => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/browser") => {
            // should the browser keep this download? (see leave_to_browser)
            let Some(body) = read_body(&mut req, 1 << 16) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            let keep = serde_json::from_str::<BrowserQuery>(&body).map(|q| take_for_browser(&q.urls)).unwrap_or(false);
            reply(req, 200, if keep { r#"{"browser":true}"# } else { r#"{"browser":false}"# })
        }
        (Method::Post, "/cli") => {
            // the command line of another start of this program (signed with its key)
            let Some(body) = read_body(&mut req, 1 << 16) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            match serde_json::from_str::<crate::cli::CliAsk>(&body) {
                Ok(ask) if ask.url.as_deref().is_none_or(|u| crate::is_download_address(u) && url::Url::parse(u).is_ok()) => {
                    on_event(ApiEvent::Cli(ask));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/open-torrent") => {
            let Some(body) = read_body(&mut req, 1 << 16) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["source"].as_str().map(String::from)) {
                Some(s) if torrent_source_ok(&s) => {
                    on_event(ApiEvent::OpenTorrent(s.trim().to_string()));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/quit") => {
            on_event(ApiEvent::Quit);
            reply(req, 200, r#"{"ok":true,"app":"Aknan"}"#)
        }
        (Method::Post, "/show") => {
            if ui_hangs() {
                let body = format!(r#"{{"ok":true,"app":"Aknan","hangs":true,"pid":{}}}"#, std::process::id());
                return reply(req, 200, &body);
            }
            on_event(ApiEvent::Show);
            reply(req, 200, r#"{"ok":true,"app":"Aknan"}"#)
        }
        (Method::Post, "/playlist") => {
            let Some(body) = read_body(&mut req, 1 << 16) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["url"].as_str().map(String::from)) {
                Some(url) if is_http(&url) => {
                    on_event(ApiEvent::Playlist(url));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/add") => {
            let Some(body) = read_body(&mut req, 1 << 20) else {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            };
            match serde_json::from_str::<AddRequest>(&body) {
                Ok(add)
                    if (is_http(&add.url) || swiftget_engine::torrent::is_magnet(&add.url))
                        && add.audio_url.as_deref().map(is_http).unwrap_or(true) =>
                {
                    on_event(ApiEvent::Add(add));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/check") => handle_check(req),
        (Method::Post, "/resolve") | (Method::Post, "/youtube") => handle_resolve(req),
        _ => reply(req, 404, r#"{"error":"not found"}"#),
    }
}

fn handle_check(mut req: Request) {
    // a download the browser started: can the program get it too?
    // Links that work once only (the browser just used them) or
    // only with the page's own sign-in (ChatGPT, iLovePDF...) stay
    // in the browser instead of failing here
    let Some(body) = read_body(&mut req, 1 << 20) else {
        return reply(req, 400, r#"{"error":"bad body"}"#);
    };
    let Ok(add) = serde_json::from_str::<AddRequest>(&body) else {
        return reply(req, 400, r#"{"error":"invalid request"}"#);
    };
    if !is_http(&add.url) {
        return reply(req, 400, r#"{"error":"not a web address"}"#);
    }
    std::thread::spawn(move || {
        let ok = can_fetch(&add);
        reply(req, 200, if ok { r#"{"ok":true}"# } else { r#"{"ok":false}"# });
    });
}

fn handle_resolve(mut req: Request) {
    // download choices for any page with a video (yt-dlp, a few
    // seconds), answered from its own thread so nothing else waits
    let Some(body) = read_body(&mut req, 4 << 20) else {
        return reply(req, 400, r#"{"error":"bad body"}"#);
    };
    let Ok(r) = serde_json::from_str::<ResolveRequest>(&body) else {
        return reply(req, 400, r#"{"error":"invalid request"}"#);
    };
    if !is_http(&r.url) {
        return reply(req, 400, r#"{"error":"not a web address"}"#);
    }
    if RESOLVING.fetch_add(1, Ordering::SeqCst) >= MAX_RESOLVING {
        RESOLVING.fetch_sub(1, Ordering::SeqCst);
        return reply(req, 429, r#"{"ok":false,"error":"busy, try again in a moment"}"#);
    }
    std::thread::spawn(move || {
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                RESOLVING.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _done = Done;
        let cookies = (!r.cookie_list.is_empty()).then_some(r.cookie_list.as_slice());
        let referer = r.referer.as_deref().filter(|u| is_http(u));
        let body = match swiftget_engine::ytdlp::resolve_with_headers(&r.url, cookies, referer, &r.headers) {
            Ok(info) => serde_json::json!({
                "ok": true, "title": info.title, "site": info.site, "options": info.options
            }),
            Err(e) => serde_json::json!({ "ok": false, "error": crate::err_text(&format!("{e:#}")) }),
        };
        reply(req, 200, &body.to_string());
    });
}
