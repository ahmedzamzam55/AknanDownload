use crate::place::Place;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The browser takes at most 1 MB from a helper in one message.
const MAX_OUT: usize = 1000 * 1024;

pub(crate) fn answer(place: &Place, msg: &Value) -> Value {
    let id = msg["id"].clone();
    let method = msg["method"].as_str().unwrap_or("GET").to_ascii_uppercase();
    let path = msg["path"].as_str().unwrap_or("");
    if path == "/hello" {
        return json!({ "id": id, "hello": true, "version": env!("CARGO_PKG_VERSION") });
    }
    if !path.starts_with('/') || !path.bytes().all(|b| b.is_ascii_graphic()) || !matches!(method.as_str(), "GET" | "POST") {
        return json!({ "id": id, "error": "bad request" });
    }
    let ms = msg["ms"].as_u64().unwrap_or(1500).clamp(300, 130_000);
    let body = msg["body"].as_str().unwrap_or("");
    let headers: Vec<(String, String)> = msg["headers"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let start = msg["start"].as_bool().unwrap_or(false);

    let mut tried_start = false;
    loop {
        let outcome = match place.link() {
            Some((port, token)) => call(port, &token, &method, path, &headers, body, Duration::from_millis(ms)),
            None => Err(Failure::Down),
        };
        match outcome {
            Ok((status, text)) if text.len() <= MAX_OUT => return json!({ "id": id, "status": status, "body": text }),
            Ok(_) => return json!({ "id": id, "error": "big" }),
            Err(Failure::Down) if start && !tried_start => {
                tried_start = true;
                if !open_program(place) {
                    return json!({ "id": id, "error": "down" });
                }
            }
            Err(Failure::Down) => return json!({ "id": id, "error": "down" }),
            Err(Failure::Slow) => return json!({ "id": id, "error": "timeout" }),
            Err(Failure::Other(e)) => return json!({ "id": id, "error": e }),
        }
    }
}

/// Opens Aknan Download (out of sight: the window of the download comes) and
/// waits until it answers.
fn open_program(place: &Place) -> bool {
    // two requests at once: the program is opened once
    static OPENING: Mutex<()> = Mutex::new(());
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    let answers = || place.link().is_some_and(|(port, token)| call(port, &token, "GET", "/ping", &[], "", Duration::from_millis(800)).is_ok());
    if answers() {
        return true;
    }
    let Some(app) = place.app.as_ref().filter(|a| a.is_file()) else { return false };
    if !spawn_program(app) {
        return false;
    }
    let until = Instant::now() + Duration::from_secs(25);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(300));
        if answers() {
            return true;
        }
    }
    false
}

/// The program on its own: the browser ends its helpers (and, on Windows,
/// what they started inside the browser's job), never the program.
fn spawn_program(app: &Path) -> bool {
    let make = || {
        let mut cmd = std::process::Command::new(app);
        cmd.arg("--from-browser")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // out of the browser's job when it lets us; else as it allows
        if make().creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn().is_ok() {
            return true;
        }
        make().creation_flags(flags).spawn().is_ok()
    }
    #[cfg(not(windows))]
    {
        make().spawn().is_ok()
    }
}

#[derive(Debug)]
enum Failure {
    /// nothing answers: the program is closed
    Down,
    /// it did not answer in time
    Slow,
    Other(String),
}

/// One request to the program on this computer.
fn call(port: u16, token: &str, method: &str, path: &str, headers: &[(String, String)], body: &str, wait: Duration) -> Result<(u16, String), Failure> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).map_err(|_| Failure::Down)?;
    let _ = s.set_read_timeout(Some(wait));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let sign_path = path.split(['?', '#']).next().unwrap_or(path);
    // X-Aknan-Helper: the program's link test can say the helper works
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nX-SwiftGet: 1\r\nX-Aknan-Helper: 1\r\n");
    for (k, v) in headers {
        let name_ok = !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let ours = ["host", "connection", "content-length", "transfer-encoding", "origin", "x-swiftget", "x-aknan-time", "x-aknan-nonce", "x-aknan-sig"];
        if name_ok && !ours.contains(&k.to_ascii_lowercase().as_str()) && !v.contains(['\r', '\n']) {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    head.push_str(&sign(token, method, sign_path));
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(body.as_bytes())).map_err(|e| Failure::Other(e.to_string()))?;
    let mut raw = Vec::new();
    if let Err(e) = s.read_to_end(&mut raw) {
        if raw.is_empty() {
            return Err(if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) { Failure::Slow } else { Failure::Other(e.to_string()) });
        }
    }
    parse_response(&raw).ok_or_else(|| Failure::Other("bad answer".into()))
}

pub(crate) fn parse_response(raw: &[u8]) -> Option<(u16, String)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let mut rest = &raw[split + 4..];
    let status: u16 = head.split_whitespace().nth(1)?.parse().ok()?;
    let lower = head.to_ascii_lowercase();
    let mut body = Vec::new();
    if lower.contains("transfer-encoding: chunked") {
        loop {
            let line_end = rest.windows(2).position(|w| w == b"\r\n")?;
            let size = usize::from_str_radix(std::str::from_utf8(&rest[..line_end]).ok()?.split(';').next()?.trim(), 16).ok()?;
            rest = &rest[line_end + 2..];
            if size == 0 {
                break;
            }
            body.extend_from_slice(rest.get(..size)?);
            rest = rest.get(size + 2..).unwrap_or(&[]);
        }
    } else if let Some(len) = lower.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()) {
        body.extend_from_slice(rest.get(..len.min(rest.len()))?);
    } else {
        body.extend_from_slice(rest);
    }
    Some((status, String::from_utf8_lossy(&body).to_string()))
}

/// The headers that sign a request with the program's key (as the add-in does).
fn sign(token: &str, method: &str, path: &str) -> String {
    use ring::rand::SecureRandom;
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string();
    let mut n = [0u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut n).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        n.copy_from_slice(&(t ^ u128::from(std::process::id())).to_le_bytes());
    }
    let nonce = hex(&n);
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, token.as_bytes());
    let sig = hex(ring::hmac::sign(&key, format!("{method}\n{path}\n{time}\n{nonce}").as_bytes()).as_ref());
    format!("X-Aknan-Time: {time}\r\nX-Aknan-Nonce: {nonce}\r\nX-Aknan-Sig: {sig}\r\n")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
