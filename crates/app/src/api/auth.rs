use super::server::{header, reply};
use super::state::{bundled_extension_version, MAIN_PORT, TOKEN};
use super::types::ApiEvent;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use swiftget_engine::sync::LockOk;
use tiny_http::Request;

static SEEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> =
    std::sync::LazyLock::new(Default::default);

/// The request was signed with the program's key (see secret.rs), lately,
/// and was not seen before.
pub(super) fn signed(req: &Request, path: &str) -> bool {
    let Some(key) = TOKEN.get().filter(|k| !k.is_empty()) else { return false };
    let (Some(time), Some(nonce), Some(sig)) = (header(req, "X-Aknan-Time"), header(req, "X-Aknan-Nonce"), header(req, "X-Aknan-Sig")) else {
        return false;
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let Ok(at) = time.parse::<i64>() else { return false };
    // made within the last minute (or a little ahead: the same clock)
    if (now - at).abs() > 60_000 || nonce.len() < 16 || nonce.len() > 64 {
        return false;
    }
    if !swiftget_engine::secret::signature_ok(key, req.method().as_str(), path, &time, &nonce, &sig) {
        return false;
    }
    // each signed request only once
    let mut seen = SEEN.lock_ok();
    seen.retain(|_, t| t.elapsed() < Duration::from_secs(130));
    seen.insert(nonce, std::time::Instant::now()).is_none()
}

/// Headers that sign a request of this program to itself (a second copy
/// asking the open one to show a torrent).
pub fn sign_headers(method: &str, path: &str) -> String {
    let key = TOKEN.get().cloned().unwrap_or_default();
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string();
    let nonce = swiftget_engine::secret::random_nonce();
    let sig = swiftget_engine::secret::request_signature(&key, method, path, &time, &nonce);
    format!("X-Aknan-Time: {time}\r\nX-Aknan-Nonce: {nonce}\r\nX-Aknan-Sig: {sig}\r\n")
}

/// The request was made to this computer by its own name. A web page that
/// points its own domain at 127.0.0.1 (DNS rebinding) sends its domain here.
pub(super) fn host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else { return true }; // HTTP/1.0: no Host at all
    let h = host.trim().to_ascii_lowercase();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.split(':').next().unwrap_or("")
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// Only these may call the API (a web page has an http(s) origin).
pub(super) fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) => {
            o.starts_with("chrome-extension://") || o.starts_with("moz-extension://") || o.starts_with("extension://")
        }
    }
}

/// A pairing is being asked about right now / the user said no (when).
static PAIRING: std::sync::Mutex<(bool, Option<std::time::Instant>)> = std::sync::Mutex::new((false, None));

/// After the user said no, pairing waits this long before it may ask again.
const AFTER_NO: Duration = Duration::from_secs(120);

/// An add-in without the key (Firefox, or one loaded from another folder)
/// asks for it: the user is asked in a window of the program; yes = it gets
/// the key. Asked once at a time, and not again for 10 minutes after a no.
pub(super) fn pair(req: Request, on_event: &impl Fn(ApiEvent)) {
    {
        let mut p = PAIRING.lock_ok();
        let refused_lately = p.1.map(|t| t.elapsed() < AFTER_NO).unwrap_or(false);
        if p.0 || refused_lately {
            return reply(req, 429, r#"{"error":"wait"}"#);
        }
        p.0 = true;
    }
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    on_event(ApiEvent::Pair(tx));
    // the answer waits for the user (on its own thread: other requests go on)
    std::thread::spawn(move || {
        // yes / no (the window closed without yes) / no answer at all
        let answer = rx.recv_timeout(Duration::from_secs(120));
        let yes = matches!(answer, Ok(true));
        {
            let mut p = PAIRING.lock_ok();
            p.0 = false;
            // only a real "no" waits before asking again; no answer does not
            if !yes && !matches!(answer, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
                p.1 = Some(std::time::Instant::now());
            }
        }
        if yes {
            let body = format!(
                r#"{{"ok":true,"token":"{}","port":{}}}"#,
                TOKEN.get().cloned().unwrap_or_default(),
                MAIN_PORT.load(Ordering::Relaxed)
            );
            reply(req, 200, &body)
        } else {
            reply(req, 403, r#"{"error":"refused"}"#)
        }
    });
}

/// The Firefox add-on that asks is older than the one of this program.
/// Add-ins say their version (X-Aknan-Ext, since 1.0.10); a Firefox add-on
/// that says nothing is from before.
pub(super) fn old_firefox_addon(req: &Request) -> bool {
    is_old_firefox(
        header(req, "X-Aknan-Browser").as_deref(),
        &header(req, "User-Agent").unwrap_or_default(),
        header(req, "X-Aknan-Ext").as_deref(),
        &bundled_extension_version(),
    )
}

pub(super) fn is_old_firefox(browser: Option<&str>, user_agent: &str, ext: Option<&str>, ours: &str) -> bool {
    let firefox = match browser {
        Some(b) => b == "firefox",
        None => user_agent.contains("Firefox/"),
    };
    if !firefox || ours.is_empty() {
        return false;
    }
    match ext {
        Some(v) => swiftget_engine::selfupdate::is_newer(ours, v.trim()),
        // no version: an add-on from before 1.0.10
        None => true,
    }
}

/// At most once every 6 hours (the user may say "later").
pub(super) fn time_to_tell_old_addon() -> bool {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 6 * 3600 {
        return false;
    }
    LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}
