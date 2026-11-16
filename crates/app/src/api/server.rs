use super::auth::{host_allowed, old_firefox_addon, origin_allowed, pair, signed, time_to_tell_old_addon};
use super::routes::dispatch;
use super::state::{now_secs, ADDIN_SEEN, HELPER_SEEN, MAIN_PORT};
use super::types::{ApiEvent, Door};
use std::sync::atomic::Ordering;
use tiny_http::{Header, Method, Request, Response, Server};

pub fn serve(
    server: Server,
    door: Door,
    config: impl Fn() -> String + Send + 'static,
    on_event: impl Fn(ApiEvent) + Send + 'static,
) {
    std::thread::Builder::new()
        .name(format!("aknan-api-{door:?}"))
        .spawn(move || {
            for req in server.incoming_requests() {
                // one request that goes wrong must not close the door for
                // good (the program would look closed to the add-in)
                let run = std::panic::AssertUnwindSafe(|| handle(req, door, &config, &on_event));
                if std::panic::catch_unwind(run).is_err() {
                    eprintln!("a request to the add-in port failed");
                }
            }
        })
        .expect("failed to start API thread");
}

pub(super) fn is_http(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

pub(super) fn header(req: &Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

pub(super) fn reply(req: Request, code: u16, body: &str) {
    let resp = Response::from_string(body)
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    let _ = req.respond(resp);
}

fn handle(req: Request, door: Door, config: &impl Fn() -> String, on_event: &impl Fn(ApiEvent)) {
    // never answer CORS preflights: web pages must not reach this API
    if *req.method() == Method::Options {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if !host_allowed(header(&req, "Host").as_deref()) {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if !origin_allowed(header(&req, "Origin").as_deref()) {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if header(&req, "X-SwiftGet").is_none() {
        return reply(req, 403, r#"{"error":"missing header"}"#);
    }

    let path = req.url().split('?').next().unwrap_or("").to_string();
    if old_firefox_addon(&req) && time_to_tell_old_addon() {
        on_event(ApiEvent::OldFirefoxAddon);
    }
    // "are you there", pairing, "show yourself" / "close" (this program and
    // its setup) are open on the known port; everything else is signed with
    // the key the add-in got from this program (another program on the
    // computer has no key), and only "where is the other port" and opening
    // a torrent may come in on the known port
    let open = matches!(path.as_str(), "/ping") || (door == Door::Front && matches!(path.as_str(), "/show" | "/quit" | "/pair"));
    if !open {
        let front_ok = matches!(path.as_str(), "/where" | "/open-torrent" | "/cli");
        if door == Door::Front && !front_ok {
            return reply(req, 404, r#"{"error":"not here","where":true}"#);
        }
        if !signed(&req, &path) {
            return reply(req, 401, r#"{"error":"pair","pair":true}"#);
        }
        // an add-in with the key says who it is: the browser's helper may be
        // started by it from now on (one without the key is never added)
        if let Some(id) = header(&req, "X-Aknan-Id") {
            crate::nativehost::saw_addin(id.trim());
        }
        ADDIN_SEEN.store(now_secs(), Ordering::Relaxed);
        if header(&req, "X-Aknan-Helper").is_some() {
            HELPER_SEEN.store(now_secs(), Ordering::Relaxed);
        }
    }
    if path == "/pair" && *req.method() == Method::Post {
        return pair(req, on_event);
    }
    if path == "/where" {
        let body = format!(r#"{{"ok":true,"port":{}}}"#, MAIN_PORT.load(Ordering::Relaxed));
        return reply(req, 200, &body);
    }
    let method = req.method().clone();
    dispatch(req, method, &path, config, on_event);
}
