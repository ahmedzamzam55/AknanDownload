use super::client::env_for;
use super::*;

#[test]
fn helper_programs_get_the_proxy() {
    let manual = ProxyConfig { mode: 2, host: "http://10.0.0.1".into(), port: 8080, user: "a b".into(), password: "p@ss".into(), bypass: "local.test  x.test".into() };
    let env = env_for(&manual);
    let get = |k: &str| env.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
    assert_eq!(get("HTTPS_PROXY").as_deref(), Some("http://a%20b:p%40ss@10.0.0.1:8080"));
    assert_eq!(get("NO_PROXY").as_deref(), Some("local.test,x.test"));
    let none = env_for(&ProxyConfig { mode: 0, ..Default::default() });
    assert!(none.iter().any(|(k, v)| *k == "http_proxy" && v.is_none()));
    assert!(none.iter().any(|(k, v)| *k == "NO_PROXY" && v.as_deref() == Some("*")));
    assert!(env_for(&ProxyConfig { mode: 1, ..Default::default() }).is_empty());
}

#[test]
fn which_failures_pass() {
    assert!(is_passing_error(&anyhow::Error::new(HttpStatus(503))));
    assert!(is_passing_error(&anyhow::Error::new(HttpStatus(429))));
    assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(404))));
    assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(403))));
    assert!(is_passing_error(&anyhow::anyhow!("download ended before all parts were received")));
    assert!(!is_passing_error(&anyhow::anyhow!("not enough room on the disk")));
    assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(410)).context("the link has expired or the file was removed")));
    // soon at first, never more than a minute
    assert_eq!(retry_delay(1).as_secs(), 3);
    assert_eq!(retry_delay(20).as_secs(), 60);
}
