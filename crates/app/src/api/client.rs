use super::auth::sign_headers;
use super::types::Existing;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tiny_http::Server;

/// Hand a command line download to the copy of the program that is open.
pub fn notify_cli(port: u16, ask: &crate::cli::CliAsk) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let body = serde_json::to_string(ask).unwrap_or_default();
    let req = format!(
        "POST /cli HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        sign_headers("POST", "/cli"),
        body.len()
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200")
}

/// Ask the copy of the program that is open to close (for the setup).
/// true = it said it will.
pub fn notify_quit(port: u16) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = "POST /quit HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.contains("Aknan")
}

pub fn bind(port: u16) -> Result<Server, String> {
    Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())
}

/// The random port of this computer: the one used last time when it is free,
/// else a new one. None = no port could be opened.
pub fn bind_main(last: u16) -> Option<(Server, u16)> {
    if last >= 1024 {
        if let Ok(s) = bind(last) {
            return Some((s, last));
        }
    }
    for _ in 0..40 {
        let p = 20000 + (u16::from_str_radix(&swiftget_engine::secret::random_nonce()[..4], 16).unwrap_or(1) % 40000);
        if let Ok(s) = bind(p) {
            return Some((s, p));
        }
    }
    None
}

/// Another Aknan Download is already running: hand it the torrent to open.
pub fn notify_open_torrent(port: u16, source: &str) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let body = serde_json::json!({ "source": source }).to_string();
    let req = format!(
        "POST /open-torrent HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        sign_headers("POST", "/open-torrent"),
        body.len()
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200")
}

/// Another SwiftGet is already running on this port: ask it to show itself.
pub fn notify_existing(port: u16) -> Existing {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return Existing::None;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return Existing::None;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    if !(buf.contains("Aknan") || buf.starts_with("HTTP/1.1 200")) {
        return Existing::None;
    }
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or_default();
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v["hangs"].as_bool() == Some(true) => Existing::Hangs(v["pid"].as_u64().unwrap_or(0) as u32),
        _ => Existing::Shown,
    }
}

/// The known port could not be asked: the copy that may be open is asked on
/// its own port (signed, as the add-in does) to come to the front.
pub fn show_on_own_port(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let req = format!(
        "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{}Content-Length: 0\r\nConnection: close\r\n\r\n",
        sign_headers("POST", "/show")
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200") && buf.contains("Aknan")
}
