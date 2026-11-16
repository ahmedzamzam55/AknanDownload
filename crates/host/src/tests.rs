use crate::client::{answer, parse_response};
use crate::place::{open_key, Place};
use crate::read_message;
use serde_json::json;

#[test]
fn answers_are_read() {
    let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
    assert_eq!(parse_response(plain), Some((200, "{\"ok\":true}".to_string())));
    let chunked = b"HTTP/1.1 401 Unauthorized\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
    assert_eq!(parse_response(chunked), Some((401, "{\"a\":1}".to_string())));
}

#[test]
fn the_key_is_read_as_the_program_keeps_it() {
    assert_eq!(open_key(&"k".repeat(64)).as_deref(), Some("k".repeat(64).as_str()));
    assert_eq!(open_key("short"), None);
    // encrypted for another user (or not at all on this system): no key
    assert_eq!(open_key("dpapi:0102"), None);
    assert_eq!(open_key("dpapi:zz"), None);
}

#[test]
fn messages_go_both_ways() {
    let v = json!({ "id": 3, "path": "/hello" });
    let data = v.to_string();
    let mut buf = (data.len() as u32).to_ne_bytes().to_vec();
    buf.extend_from_slice(data.as_bytes());
    let got = read_message(&mut buf.as_slice()).unwrap();
    assert_eq!(got["id"], 3);
    let a = answer(&Place { app: None, settings: None }, &got);
    assert_eq!(a["hello"], true);
    // nothing to talk to: "down" (not opened, nothing was asked to start)
    let a = answer(&Place { app: None, settings: None }, &json!({ "id": 4, "path": "/ping" }));
    assert_eq!(a["error"], "down");
}
