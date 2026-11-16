use crate::helpers::{serve, test_data};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::http::{build_client, probe, Progress, Shared};
use swiftget_engine::http_download::{self, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_with_pause_and_resume() {
    let data = test_data(4 * 1024 * 1024 + 123);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));

    let p = probe(&shared.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(p.total_size, Some(data.len() as u64));
    assert!(p.resumable);
    assert_eq!(p.file_name, "test file.bin");

    let dir = std::env::temp_dir().join(format!("swiftget-test-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let _ = std::fs::remove_file(&part);
    let job = HttpJob {
        url: url.clone(),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: p.total_size,
        resumable: true,
        part_path: part.clone(),
        connections: 8,
        min_split: 64 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check: Default::default(),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());

    // start, then pause once about a quarter is there
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let p2 = progress.clone();
    let quarter = data.len() as u64 / 4;
    tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < quarter {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        c2.cancel();
    });
    let first = http_download::run(shared.clone(), job.clone(), segs.clone(), progress.clone(), cancel).await;
    assert!(first.is_err(), "should have been paused");
    let after_pause = progress.bytes.load(Ordering::Relaxed);
    assert!(after_pause > 0 && after_pause < data.len() as u64);

    // resume with the saved segment table
    let saved = segs.lock().unwrap().clone();
    let segs = Arc::new(Mutex::new(saved));
    http_download::run(shared, job, segs.clone(), progress.clone(), CancellationToken::new())
        .await
        .unwrap();

    let out = std::fs::read(&part).unwrap();
    assert_eq!(out.len(), data.len());
    assert!(out == *data, "downloaded bytes differ from the source");
    // how far the file gets split depends on how fast the machine is
    assert!(!segs.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// YouTube style: each connection may only ask for a small piece per request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunked_requests_with_32_connections() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/v.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let dir = std::env::temp_dir().join(format!("swiftget-chunk-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("v.sgpart");
    let job = HttpJob {
        url,
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(data.len() as u64),
        resumable: true,
        part_path: part.clone(),
        connections: 32,
        min_split: 32 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: Some(40 * 1024),
        limiter: Arc::new(SpeedLimiter::new(0)),
        check: Default::default(),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new()).await.unwrap();
    let out = std::fs::read(&part).unwrap();
    assert!(out == *data, "chunked download differs from the source");
    assert_eq!(progress.bytes.load(Ordering::Relaxed), data.len() as u64);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Options > connections changed while a file downloads: fewer, then more,
/// at once and without a restart; the file is still byte for byte right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_changed_while_downloading() {
    let data = test_data(6 * 1024 * 1024 + 77);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/conns.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let p = probe(&shared.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    let dir = std::env::temp_dir().join(format!("swiftget-conns-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let _ = std::fs::remove_file(&part);
    let job = HttpJob {
        url,
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: p.total_size,
        resumable: true,
        part_path: part.clone(),
        connections: 8,
        min_split: 64 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check: Default::default(),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());
    let p2 = progress.clone();
    let total = data.len() as u64;
    let changer = tokio::spawn(async move {
        let mut fewest = u32::MAX;
        let mut most = 0;
        // 8 -> 2 at a fifth of the file
        while p2.bytes.load(Ordering::Relaxed) < total / 5 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        p2.want_connections.store(2, Ordering::Relaxed);
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            fewest = fewest.min(p2.connections.load(Ordering::Relaxed));
            if p2.bytes.load(Ordering::Relaxed) > total / 2 {
                break;
            }
        }
        // then 16
        p2.want_connections.store(16, Ordering::Relaxed);
        for _ in 0..600 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            most = most.max(p2.connections.load(Ordering::Relaxed));
        }
        (fewest, most)
    });
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new()).await.unwrap();
    let (fewest, most) = changer.await.unwrap();
    assert!(fewest <= 2, "fewer connections at once: {fewest}");
    assert!(most > 8, "more connections at once: {most}");
    assert_eq!(std::fs::read(&part).unwrap(), *data, "the file is right");
    let _ = std::fs::remove_dir_all(&dir);
}
