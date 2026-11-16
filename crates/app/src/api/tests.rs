use super::auth::{host_allowed, is_old_firefox, origin_allowed, sign_headers};
use super::probe::can_fetch;
use super::types::AddRequest;
use super::*;
use std::io::{Read, Write};
use std::net::TcpStream;

#[test]
fn an_old_firefox_addon_is_told() {
    let ff = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:145.0) Gecko/20100101 Firefox/145.0";
    let chrome = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
    // an add-on from before (no version): Firefox only
    assert!(is_old_firefox(None, ff, None, "1.0.10"));
    assert!(!is_old_firefox(None, chrome, None, "1.0.10"));
    // the add-on says its version
    assert!(is_old_firefox(Some("firefox"), ff, Some("1.0.9"), "1.0.10"));
    assert!(!is_old_firefox(Some("firefox"), ff, Some("1.0.10"), "1.0.10"));
    assert!(!is_old_firefox(Some("firefox"), ff, Some("1.0.11"), "1.0.10"));
    assert!(!is_old_firefox(Some("chromium"), chrome, Some("1.0.8"), "1.0.10"));
    assert!(!is_old_firefox(None, ff, None, ""));
}

#[test]
fn the_two_doors() {
    let _ = TOKEN.set("a".repeat(64));
    let front = bind(0).unwrap();
    let fport = front.server_addr().to_ip().unwrap().port();
    let main = bind(0).unwrap();
    let mport = main.server_addr().to_ip().unwrap().port();
    MAIN_PORT.store(mport, std::sync::atomic::Ordering::Relaxed);
    serve(front, Door::Front, || "{}".to_string(), |_| {});
    serve(main, Door::Main, || r#"{"ok":true}"#.to_string(), |_| {});
    let ask = |port: u16, path: &str, signed: Option<String>| -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let sig = signed.unwrap_or_default();
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{sig}Connection: close\r\n\r\n");
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    // unsigned: refused on the real port; the known port has no /config
    assert!(ask(mport, "/config", None).starts_with("HTTP/1.1 401"));
    assert!(ask(fport, "/config", Some(sign_headers("GET", "/config"))).starts_with("HTTP/1.1 404"));
    // signed: taken, once
    let h = sign_headers("GET", "/config");
    assert!(ask(mport, "/config", Some(h.clone())).starts_with("HTTP/1.1 200"));
    assert!(ask(mport, "/config", Some(h)).starts_with("HTTP/1.1 401"), "a signed request is taken only once");
    // signed for another path: refused
    assert!(ask(mport, "/config", Some(sign_headers("GET", "/links"))).starts_with("HTTP/1.1 401"));
    // where the real port is: only with a signature
    assert!(ask(fport, "/where", None).starts_with("HTTP/1.1 401"));
    assert!(ask(fport, "/where", Some(sign_headers("GET", "/where"))).contains(&format!("\"port\":{mport}")));
    // "are you there" stays open on both
    assert!(ask(fport, "/ping", None).starts_with("HTTP/1.1 200"));
}

/// The browser's helper (crates/host) talks to the program as the add-in
/// does: run with AKNAN_HOST_BIN = the built helper.
#[test]
fn the_helper_gets_through() {
    let Some(bin) = std::env::var_os("AKNAN_HOST_BIN") else { return };
    let _ = TOKEN.set("a".repeat(64));
    let token = TOKEN.get().unwrap().clone();
    let main = bind(0).unwrap();
    let mport = main.server_addr().to_ip().unwrap().port();
    serve(main, Door::Main, || r#"{"ok":true,"from":"config"}"#.to_string(), |_| {});
    let dir = std::env::temp_dir().join(format!("aknan-helper-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let host = dir.join("aknan-host");
    std::fs::copy(&bin, &host).unwrap();
    std::fs::write(dir.join("settings.json"), format!(r#"{{"api_port":{mport},"api_token":"{token}"}}"#)).unwrap();
    std::fs::write(dir.join("app.json"), serde_json::json!({ "settings": dir.join("settings.json") }).to_string()).unwrap();
    let mut child = std::process::Command::new(&host)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut ask = |v: serde_json::Value| -> serde_json::Value {
        let d = v.to_string();
        input.write_all(&(d.len() as u32).to_ne_bytes()).unwrap();
        input.write_all(d.as_bytes()).unwrap();
        input.flush().unwrap();
        let mut len = [0u8; 4];
        output.read_exact(&mut len).unwrap();
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        output.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    };
    let a = ask(serde_json::json!({ "id": 1, "method": "GET", "path": "/config", "ms": 3000 }));
    assert_eq!(a["id"], 1);
    assert_eq!(a["status"], 200, "{a}");
    assert!(a["body"].as_str().unwrap().contains("config"));
    let a = ask(serde_json::json!({ "id": 2, "method": "GET", "path": "/ping", "ms": 3000 }));
    assert_eq!(a["status"], 200);
    assert!(a["body"].as_str().unwrap().contains("AknanDownload"));
    let a = ask(serde_json::json!({ "id": 3, "method": "POST", "path": "/nothing", "body": "{}", "ms": 3000 }));
    assert_eq!(a["status"], 404);
    // a wrong key: the program says no
    std::fs::write(dir.join("settings.json"), format!(r#"{{"api_port":{mport},"api_token":"{}"}}"#, "b".repeat(64))).unwrap();
    let a = ask(serde_json::json!({ "id": 4, "method": "GET", "path": "/config", "ms": 3000 }));
    assert_eq!(a["status"], 401);
    drop(input);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn links_the_program_can_not_get_stay_in_the_browser() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten().take(20) {
            let mut s = s;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let answer = if head.starts_with("GET /gone") {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else if head.starts_with("GET /signin") {
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n0123456789".to_string()
            };
            let _ = s.write_all(answer.as_bytes());
        }
    });
    let ask = |path: &str| {
        let a: AddRequest = serde_json::from_str(&format!(r#"{{"url":"http://127.0.0.1:{port}{path}"}}"#)).unwrap();
        can_fetch(&a)
    };
    assert!(ask("/file.zip"));
    assert!(!ask("/gone/file.zip"), "a link that works once only");
    assert!(!ask("/signin/file.zip"), "a link that needs the page's sign-in");
}

#[test]
fn a_part_of_a_video_from_the_add_in() {
    let a: AddRequest = serde_json::from_str(r#"{"url":"https://x.test/v.mp4","clip":{"start":5,"end":9.5,"exact":true}}"#).unwrap();
    let c = a.clip.unwrap();
    assert_eq!((c.start, c.end, c.exact), (5.0, 9.5, true));
    let b: AddRequest = serde_json::from_str(r#"{"url":"https://x.test/v.mp4","clip":null}"#).unwrap();
    assert!(b.clip.is_none());
}

#[test]
fn signed_requests_only_once() {
    let _ = TOKEN.set("a".repeat(64));
    let h = sign_headers("POST", "/add");
    assert!(h.contains("X-Aknan-Sig: ") && !h.contains(&"a".repeat(64)), "the key itself is never sent");
}

#[test]
fn only_this_computer_by_its_own_name() {
    for ok in [None, Some("127.0.0.1:9614"), Some("localhost:9614"), Some("127.0.0.1"), Some("[::1]:9614")] {
        assert!(host_allowed(ok), "{ok:?}");
    }
    // DNS rebinding: the attacker's own name pointed at 127.0.0.1
    for bad in [Some("evil.example:9614"), Some("127.0.0.1.evil.example"), Some("localhost.evil:9614"), Some("")] {
        assert!(!host_allowed(bad), "{bad:?}");
    }
}

#[test]
fn only_browser_add_ins() {
    assert!(origin_allowed(None));
    assert!(origin_allowed(Some("chrome-extension://abcdef")));
    assert!(origin_allowed(Some("moz-extension://1234-5678")));
    assert!(!origin_allowed(Some("https://evil.example")));
    assert!(!origin_allowed(Some("null")));
}
