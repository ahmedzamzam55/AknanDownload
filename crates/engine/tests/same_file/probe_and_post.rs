use crate::helpers::{bytes, job, ranged, serve, shared, temp, wait_end, Asked};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use swiftget_engine::http::{self, probe, Progress};
use swiftget_engine::http_download::{self, FileCheck};
use swiftget_engine::RequestHeaders;
use tokio_util::sync::CancellationToken;

/// A link that works for one request only: the look keeps its answer, a
/// second look asks nothing, and the download reads the file from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_for_one_request_is_not_spent_by_the_look() {
    let size = 600 * 1024;
    let data = Arc::new(bytes(size, 4));
    let asked = Arc::new(AtomicU32::new(0));
    let (d, a2) = (data.clone(), asked.clone());
    let port = serve(Arc::new(move |a: &Asked| {
        if a2.fetch_add(1, Ordering::Relaxed) == 0 {
            ranged(&d, a, None)
        } else {
            ("403 Forbidden".into(), vec![], vec![])
        }
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/once.bin");
    let s = shared();
    let p = probe(&s.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(p.total_size, Some(size as u64));
    // the window looks again: the kept answer says it
    let again = probe(&s.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(again.total_size, Some(size as u64));
    assert_eq!(asked.load(Ordering::Relaxed), 1, "asked once only");
    let (_, answer) = http::take_kept(&url).expect("kept");
    let part = temp("once", port);
    let j = job(&url, size, &part, 1, FileCheck::new(None, Some(answer)));
    http_download::run(s, j, Arc::new(Mutex::new(Vec::new())), Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert!(std::fs::read(&part).unwrap() == *data);
    assert_eq!(asked.load(Ordering::Relaxed), 1, "the download asked nothing more");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// "Your download starts in a moment": a page that leads on to the file
/// (in a meta tag, or the "Refresh" header) with a cookie it sets.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pages_that_lead_on_to_the_file_are_followed() {
    let data = Arc::new(bytes(100 * 1024, 5));
    let d = data.clone();
    let port = serve(Arc::new(move |a: &Asked| match a.path.as_str() {
        "/wait" => (
            "200 OK".into(),
            vec!["Content-Type: text/html; charset=utf-8".into(), "Set-Cookie: ticket=42; Path=/".into()],
            b"<html><head><META HTTP-EQUIV='Refresh' CONTENT='1; URL=/second?a=1&amp;b=2'></head><body>wait</body></html>".to_vec(),
        ),
        "/second?a=1&b=2" => ("200 OK".into(), vec!["Content-Type: text/html".into(), "Refresh: 0; url=/file.zip".into()], b"<html></html>".to_vec()),
        "/file.zip" if a.cookie.contains("ticket=42") => ranged(&d, a, None),
        _ => ("403 Forbidden".into(), vec![], vec![]),
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/wait");
    let p = probe(&shared().client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert!(p.final_url.ends_with("/file.zip"), "{}", p.final_url);
    assert_eq!(p.total_size, Some(data.len() as u64));
    assert!(p.headers_used.unwrap().cookies.unwrap().contains("ticket=42"));
    http::drop_kept(&url);
}

/// A site's "not a robot" check: told as such, not asked again and again.
#[tokio::test]
async fn a_robot_check_is_told() {
    let asked = Arc::new(AtomicU32::new(0));
    let a2 = asked.clone();
    let port = serve(Arc::new(move |_: &Asked| {
        a2.fetch_add(1, Ordering::Relaxed);
        ("403 Forbidden".into(), vec!["cf-mitigated: challenge".into(), "Content-Type: text/html".into()], b"<html>check</html>".to_vec())
    }))
    .await;
    let e = probe(&shared().client(), &format!("http://127.0.0.1:{port}/f.zip"), &RequestHeaders::default(), "test").await.unwrap_err();
    assert!(http::wants_captcha(&e), "{e:#}");
    assert_eq!(asked.load(Ordering::Relaxed), 1);
}

#[test]
fn refresh_values() {
    let base = url::Url::parse("https://a.test/dir/page?x=1").unwrap();
    let (w, u) = http::refresh_target("5; url=next.html", &base).unwrap();
    assert_eq!((w.as_secs(), u.as_str()), (5, "https://a.test/dir/next.html"));
    let (w, u) = http::refresh_target("0;URL='https://b.test/f.zip'", &base).unwrap();
    assert_eq!((w.as_secs(), u.as_str()), (0, "https://b.test/f.zip"));
    // a long wait is cut short; a page reloading itself leads nowhere
    assert_eq!(http::refresh_target("90, /f", &base).unwrap().0.as_secs(), 10);
    assert!(http::refresh_target("3", &base).is_none());
    assert!(http::refresh_target("3; url=page?x=1", &base).is_none());
    assert!(http::refresh_target("1; url=javascript:go()", &base).is_none());
    let page = r#"<meta charset="utf-8"><meta content="2;url=/go" http-equiv="refresh">"#;
    assert_eq!(http::meta_refresh(page, &base).unwrap().1.as_str(), "https://a.test/go");
}

/// A checksum the user pasted is checked before the file is done: right, it
/// is done; wrong, the download says the file is damaged. A file of the same
/// name that is already there is never replaced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_checksum_is_checked_and_nothing_is_replaced() {
    let data = Arc::new(b"abc".repeat(100_000));
    let d = data.clone();
    let port = serve(Arc::new(move |a: &Asked| ranged(&d, a, None))).await;
    let root = std::env::temp_dir().join(format!("aknan-sum-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.bin"), b"mine").unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let sha = {
        let mut c = ring::digest::Context::new(&ring::digest::SHA256);
        c.update(&data);
        c.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let add = |checksum: &str| swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/f.bin"),
        save_dir: Some(dir.clone()),
        file_name: Some("f.bin".into()),
        start: true,
        checksum: Some(checksum.to_string()),
        ..Default::default()
    };
    let good = wait_end(&mgr, mgr.add(add(&format!("SHA-256: {sha}")))).await;
    assert_eq!(good.status, swiftget_engine::Status::Completed, "{:?}", good.error);
    assert!(std::fs::read(&good.save_path).unwrap() == *data);
    assert_eq!(std::fs::read(dir.join("f.bin")).unwrap(), b"mine", "the file that was there stays");
    let bad = wait_end(&mgr, mgr.add(add("900150983cd24fb0d6963f7d28e17f72"))).await;
    assert_eq!(bad.status, swiftget_engine::Status::Failed);
    assert!(bad.error.unwrap_or_default().contains("damaged"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A form's download: asked with POST and the form's data, and the file
/// comes in parts on many connections when the server takes ranges.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_form_download_is_posted() {
    let data = Arc::new(bytes(1024 * 1024, 6));
    let d = data.clone();
    let port = serve(Arc::new(move |a: &Asked| {
        if a.method == "POST" && a.body == "id=7&ok=1" {
            ranged(&d, a, None)
        } else {
            ("405 Method Not Allowed".into(), vec![], vec![])
        }
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/export");
    let h = RequestHeaders { post: Some("aWQ9NyZvaz0x".into()), ..Default::default() };
    let p = probe(&shared().client(), &url, &h, "test").await.unwrap();
    assert!(p.resumable);
    http::drop_kept(&url);
    let part = temp("form", port);
    let mut j = job(&url, data.len(), &part, 4, FileCheck::default());
    j.headers = h;
    http_download::run(shared(), j, Arc::new(Mutex::new(Vec::new())), Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert!(std::fs::read(&part).unwrap() == *data);
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}
