use crate::limiter::SpeedLimiter;
use crate::sync::RwOk;
use reqwest::Client;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Live counters of a running download, read by the UI.
#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub connections: AtomicU32,
    /// torrents: how many of the connected people have the whole file
    pub seeders: AtomicU32,
    pub parts_total: AtomicU32,
    pub parts_done: AtomicU32,
    pub assembling: AtomicBool,
    /// the connection dropped: waiting for the internet to come back
    pub waiting: AtomicBool,
    /// woken when data flows again, so every waiting part retries at once
    pub back_online: tokio::sync::Notify,
    /// connections the user wants now (0 = as many as the download began
    /// with): changed while it runs, it gets more / fewer at once
    pub want_connections: AtomicU32,
    /// when the server last made it take fewer connections (seconds since
    /// 1970): a while without that, one more is tried again
    pub lowered_at: std::sync::atomic::AtomicU64,
    /// after the data: what it does (Step code, 0 = nothing) and how far (per mille)
    pub step: std::sync::atomic::AtomicU8,
    pub step_done: AtomicU32,
    /// streams whose exact size is estimated from completed parts/duration
    pub est_total: AtomicU64,
    /// per-worker (downloaded_bytes, active) for HLS connection display
    pub hls_workers: [(AtomicU64, AtomicBool); 32],
}

impl Progress {
    /// Now doing `step` (None: nothing more).
    pub fn set_step(&self, step: Option<crate::model::Step>) {
        self.step.store(step.map(|s| s.code()).unwrap_or(0), Ordering::Relaxed);
        self.step_done.store(0, Ordering::Relaxed);
    }
    /// How far the step is (0..1).
    pub fn step_at(&self, f: f64) {
        self.step_done.store((f.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    pub fn reset_hls_workers(&self) {
        for (b, a) in &self.hls_workers {
            b.store(0, Ordering::Relaxed);
            a.store(false, Ordering::Relaxed);
        }
    }

    /// Data arrived: the connection works (again).
    pub fn online(&self) {
        if self.waiting.swap(false, Ordering::Relaxed) {
            self.back_online.notify_waiters();
        }
    }
}

/// The connection broke while receiving (stall, early close).
#[derive(Debug)]
pub struct NetDrop(pub &'static str);
impl std::fmt::Display for NetDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for NetDrop {}

/// The server answered with an error status (optional `Retry-After`).
#[derive(Debug)]
pub struct HttpStatus(pub u16);
impl std::fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "server answered HTTP {}", self.0) }
}
impl std::error::Error for HttpStatus {}

/// Error status accompanied by an optional server `Retry-After` delay.
#[derive(Debug)]
pub struct HttpRetry(pub u16, pub Option<Duration>);
impl std::fmt::Display for HttpRetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "server answered HTTP {}", self.0) }
}
impl std::error::Error for HttpRetry {}

/// A problem of the connection (no internet, DNS, reset, timeout), not of
/// the file or the link: worth waiting for and trying again without limit.
pub fn is_network_error(e: &anyhow::Error) -> bool {
    use std::io::ErrorKind as K;
    e.chain().any(|c| {
        if c.is::<NetDrop>() {
            return true;
        }
        if let Some(r) = c.downcast_ref::<reqwest::Error>() {
            return r.status().is_none() && !r.is_builder() && !r.is_redirect()
                && (r.is_connect() || r.is_timeout() || r.is_request() || r.is_body() || r.is_decode());
        }
        if let Some(io) = c.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                K::ConnectionReset | K::ConnectionAborted | K::ConnectionRefused | K::NotConnected
                    | K::TimedOut | K::BrokenPipe | K::UnexpectedEof | K::HostUnreachable
                    | K::NetworkUnreachable | K::NetworkDown
            );
        }
        false
    })
}

/// The server's status code, if the error is one.
pub fn http_status_of(e: &anyhow::Error) -> Option<u16> {
    e.chain().find_map(|c| {
        c.downcast_ref::<HttpStatus>()
            .map(|s| s.0)
            .or_else(|| c.downcast_ref::<HttpRetry>().map(|r| r.0))
    })
}

/// Optional `Retry-After` duration attached to an HTTP error.
pub fn retry_after_of(e: &anyhow::Error) -> Option<Duration> {
    e.chain().find_map(|c| c.downcast_ref::<HttpRetry>().and_then(|r| r.1))
}

/// How long to wait before trying again after `n` failures in a row while
/// the connection is down: quick at first, then every 10 seconds.
pub fn reconnect_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 2,
        2 => 3,
        3 => 5,
        4 => 8,
        _ => 10,
    })
}

/// A failure of the server or the transfer (busy server, "HTTP 503", a
/// connection that broke off early) that may well go away by itself: the
/// download tries again later on its own. Not: a link that expired or a
/// file that was removed (401/403/404/410), a login, a full disk.
pub fn is_passing_error(e: &anyhow::Error) -> bool {
    if is_network_error(e) {
        return true;
    }
    if let Some(s) = http_status_of(e) {
        return s == 408 || s == 425 || s == 429 || (500..600).contains(&s);
    }
    let text = format!("{e:#}").to_ascii_lowercase();
    let lasting = [
        "expired", "removed", "login", "age check", "not enough", "no room", "disk", "space",
        "access is denied", "permission", "bitlocker", "not found", "did not accept the byte range",
        "web page", "missing", "cannot", "can't", "invalid", "changed since",
    ];
    if lasting.iter().any(|w| text.contains(w)) {
        return false;
    }
    ["closed early", "ended before", "timed out", "reset", "broken pipe", "unexpected eof", "connection"]
        .iter()
        .any(|w| text.contains(w))
}

/// The server stopped taking the link (401 / 403 / 410, an expired link):
/// a new link is fetched the way the first one was (the address the
/// download came from, or the video's page), checked, and the download goes
/// on with it from where it was.
/// The site wants a "not a robot" check in the browser first.
pub const CAPTCHA: &str = "the site asks to confirm you are not a robot: open the link in the browser, confirm, then resume the download";

pub fn wants_captcha(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains(CAPTCHA)
}

pub fn is_link_gone(e: &anyhow::Error) -> bool {
    if wants_captcha(e) {
        return false;
    }
    if matches!(http_status_of(e), Some(401 | 403 | 410)) {
        return true;
    }
    let text = format!("{e:#}").to_ascii_lowercase();
    text.contains("link has expired") || text.contains("not the same file")
}

/// How long to wait before the next try to get a working link: 2 s, 5 s,
/// 15 s, 30 s, 1 min, 2 min.
pub fn relink_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 2,
        2 => 5,
        3 => 15,
        4 => 30,
        5 => 60,
        _ => 120,
    })
}

/// How long to wait before the next try after a passing failure: soon at
/// first (a download that stood still for minutes with "trying again in
/// 10 min" was started by hand with Pause / Resume): 3 s, 5 s, 10 s, 20 s,
/// 30 s, then every minute.
pub fn retry_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 3,
        2 => 5,
        3 => 10,
        4 => 20,
        5 => 30,
        _ => 60,
    })
}

/// Counts an open connection for as long as it lives.
pub struct ConnGuard(Arc<Progress>);
impl ConnGuard {
    pub fn new(p: &Arc<Progress>) -> Self {
        p.connections.fetch_add(1, Ordering::Relaxed);
        ConnGuard(p.clone())
    }
}
impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Things every transfer needs.
pub struct Shared {
    /// replaced when the proxy settings change
    client: RwLock<Client>,
    pub limiter: SpeedLimiter,
}

impl Shared {
    pub fn new(client: Client, limiter: SpeedLimiter) -> Shared {
        Shared { client: RwLock::new(client), limiter }
    }
    /// The HTTP client (cheap to clone: it is shared inside).
    pub fn client(&self) -> Client {
        self.client.read_ok().clone()
    }
    pub fn set_client(&self, c: Client) {
        *self.client.write_ok() = c;
    }
}
