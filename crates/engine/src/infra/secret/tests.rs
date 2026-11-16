#[test]
fn request_signatures() {
    let k = "k".repeat(64);
    let sig = super::request_signature(&k, "POST", "/add", "1700000000000", "abc");
    assert_eq!(sig.len(), 64);
    // the same as the browser add-in makes (crypto.subtle HMAC-SHA256)
    assert_eq!(sig, "7c71547e607fc59ffae12e99587c50ac1ac6db383cd4eacb3cd15becff7cef06");
    assert!(super::signature_ok(&k, "POST", "/add", "1700000000000", "abc", &sig));
    assert!(!super::signature_ok(&k, "POST", "/links", "1700000000000", "abc", &sig));
    assert!(!super::signature_ok(&k, "POST", "/add", "1700000000001", "abc", &sig));
    assert!(!super::signature_ok(&"x".repeat(64), "POST", "/add", "1700000000000", "abc", &sig));
    assert!(!super::signature_ok(&k, "POST", "/add", "1700000000000", "abc", "zz"));
}

use super::*;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct T {
    #[serde(with = "string")]
    p: String,
    #[serde(default, with = "opt")]
    c: Option<String>,
    #[serde(default, with = "pair")]
    a: Option<(String, String)>,
}

#[test]
fn plain_text_of_old_files_still_reads() {
    let t: T = serde_json::from_str(r#"{"p":"pw","c":"k=v","a":["u","x"]}"#).unwrap();
    assert_eq!(t, T { p: "pw".into(), c: Some("k=v".into()), a: Some(("u".into(), "x".into())) });
    let back: T = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(back, t);
}

#[test]
fn unreadable_protected_text_is_empty_not_garbage() {
    assert_eq!(open("dpapi:zz"), "");
    assert_eq!(open("dpapi:00ff"), ""); // not this user's key
    let t: T = serde_json::from_str(r#"{"p":"dpapi:1234","c":"dpapi:","a":null}"#).unwrap();
    assert_eq!(t.p, "");
    assert_eq!(t.c, None);
    assert_eq!(seal(""), "");
}
