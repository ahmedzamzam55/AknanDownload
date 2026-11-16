use super::learned::*;
use super::types::parse_content_range;
use crate::http::Progress;
use std::sync::atomic::Ordering;

#[test]
fn sites_and_their_limits() {
    assert_eq!(site_of("https://download2430.mediafire.com/x/y.rar").as_deref(), Some("mediafire.com"));
    assert_eq!(site_of("https://a.b.example.co.uk/f").as_deref(), Some("example.co.uk"));
    assert_eq!(site_of("http://127.0.0.1:8080/f").as_deref(), Some("127.0.0.1"));
    learn("https://download1.mediafire.test/a", 8);
    learn("https://download2.mediafire.test/b", 12);
    assert_eq!(learned_limit("https://download9.mediafire.test/c"), Some(8));
    learn("https://download2.mediafire.test/b", 5);
    assert_eq!(learned_limit("https://x.mediafire.test/"), Some(5));
    // a limit from a file of an older version (no time): let go
    let old: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(r#"{"old.test": 1, "new.test": [3, 9999999999], "gone.test": [2, 1]}"#).unwrap();
    set_learned(old);
    assert_eq!(learned_limit("https://old.test/"), None);
    assert_eq!(learned_limit("https://gone.test/"), None, "older than a day");
    let p = Progress::default();
    lower_wanted(&p, 32, 31);
    lower_wanted(&p, 32, 40);
    assert_eq!(p.want_connections.load(Ordering::Relaxed), 31);
}


#[test]
fn content_range() {
    assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
    assert_eq!(parse_content_range("bytes 0-0/*"), Some((0, None)));
    assert_eq!(parse_content_range(" bytes  5-9/10 "), Some((5, Some(10))));
    assert_eq!(parse_content_range("bytes */1000"), None);
    assert_eq!(parse_content_range("items 1-2/3"), None);
}
