use crate::helpers::{bytes, job, ranged, run_a_quarter, serve, shared, temp, Asked};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use swiftget_engine::http::Progress;
use swiftget_engine::http_download::{self, FileCheck};
use tokio_util::sync::CancellationToken;

/// The file changed on the server during a pause (same size, other bytes,
/// no version name): the bytes before each part are compared, and the
/// download stops instead of mixing the two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_changed_without_a_version_name_is_caught() {
    let size = 2 * 1024 * 1024;
    let current = Arc::new(Mutex::new(bytes(size, 1)));
    let c = current.clone();
    let port = serve(Arc::new(move |a: &Asked| ranged(&c.lock().unwrap(), a, None))).await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("plain", port);
    let j = job(&url, size, &part, 4, FileCheck::default());
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    *current.lock().unwrap() = bytes(size, 2);
    let e = http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap_err();
    assert!(format!("{e:#}").contains("changed since"), "{e:#}");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// The same with a version name: the server sends the whole new file to
/// "If-Range" of the old one, and the download stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_version_is_caught_by_its_name() {
    let size = 2 * 1024 * 1024;
    let current = Arc::new(Mutex::new((bytes(size, 1), "\"v1\"")));
    let c = current.clone();
    let port = serve(Arc::new(move |a: &Asked| {
        let (d, e) = c.lock().unwrap().clone();
        ranged(&d, a, Some(e))
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("named", port);
    let j = job(&url, size, &part, 4, FileCheck::new(Some("\"v1\"".into()), None));
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    *current.lock().unwrap() = (bytes(size + 1000, 2), "\"v2\"");
    let e = http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap_err();
    assert!(format!("{e:#}").contains("changed since"), "{e:#}");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// Servers of one site that name the same file each their own way: the name
/// is let go, and the file comes whole and right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_server_naming_the_file_its_own_way_is_fine() {
    let size = 2 * 1024 * 1024;
    let data = Arc::new(bytes(size, 3));
    let n = Arc::new(AtomicU32::new(0));
    let (d, n2) = (data.clone(), n.clone());
    let port = serve(Arc::new(move |a: &Asked| {
        let tag = format!("\"node{}\"", n2.fetch_add(1, Ordering::Relaxed));
        ranged(&d, a, Some(&tag))
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("nodes", port);
    let j = job(&url, size, &part, 4, FileCheck::new(Some("\"node0\"".into()), None));
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert!(std::fs::read(&part).unwrap() == *data);
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}
