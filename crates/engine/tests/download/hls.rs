use crate::helpers::test_data;
use std::sync::Arc;
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// HLS: the list address sends the player on to another folder (relative
/// addresses are relative to that one), and the parts are byte ranges of one
/// file with an init part (#EXT-X-MAP / #EXT-X-BYTERANGE).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_byte_ranges_after_a_redirect() {
    let media = test_data(300_000);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let media = media.clone();
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let media = media.clone();
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let (mut path, mut range) = (String::new(), None::<(usize, usize)>);
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if path.is_empty() {
                            path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = v.trim().split_once('-').unwrap();
                            range = Some((a.parse().unwrap(), b.parse().unwrap()));
                        }
                    }
                    let send = |status: &str, body: &[u8], extra: String| {
                        let mut v = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", body.len()).into_bytes();
                        v.extend_from_slice(body);
                        v
                    };
                    let out = match path.as_str() {
                        "/watch/list.m3u8" => send("302 Found", b"", "Location: /cdn/v1/master.m3u8\r\n".into()),
                        "/cdn/v1/master.m3u8" => send("200 OK", b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=900000\nhd/index.m3u8\n", String::new()),
                        "/cdn/v1/hd/index.m3u8" => send(
                            "200 OK",
                            b"#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"1000@0\"\n#EXTINF:6,\n#EXT-X-BYTERANGE:99000@1000\nall.mp4\n#EXTINF:6,\n#EXT-X-BYTERANGE:100000\nall.mp4\n#EXTINF:6,\n#EXT-X-BYTERANGE:100000\nall.mp4\n#EXT-X-ENDLIST\n",
                            String::new(),
                        ),
                        "/cdn/v1/hd/all.mp4" => match range {
                            Some((a, b)) => send("206 Partial Content", &media[a..=b], format!("Content-Range: bytes {a}-{b}/{}\r\n", media.len())),
                            None => send("200 OK", &media, String::new()),
                        },
                        _ => send("404 Not Found", b"", String::new()),
                    };
                    let _ = w.write_all(&out).await;
                });
            }
        });
    }
    let dir = std::env::temp_dir().join(format!("aknan-hls-range-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let job = swiftget_engine::hls::HlsJob {
        url: format!("http://127.0.0.1:{port}/watch/list.m3u8"),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        output: dir.join("out.mp4"),
        temp_dir: dir.join("parts"),
        connections: 3,
        retries: 2,
        limiter: Arc::new(SpeedLimiter::new(0)),
        clip: None,
    };
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    swiftget_engine::hls::run(shared, job, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    let got = std::fs::read(dir.join("out.mp4")).unwrap();
    assert!(got == *media, "the stream put together is the file ({} of {} bytes)", got.len(), media.len());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_throttles_on_503_and_resumes_cleanly() {
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    let seg_len = 40_000usize;
    let base = test_data(seg_len);
    let parts_data: Arc<Vec<Vec<u8>>> = Arc::new((0..12).map(|i| base.iter().map(|b| *b ^ i as u8).collect()).collect());
    let serves: Arc<Vec<AtomicU32>> = Arc::new((0..12).map(|_| AtomicU32::new(0)).collect());
    let hit_503 = Arc::new(AtomicUsize::new(0));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.2:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let (parts_data, serves, hit_503, in_flight) = (parts_data.clone(), serves.clone(), hit_503.clone(), in_flight.clone());
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let (parts_data, serves, hit_503, in_flight) = (parts_data.clone(), serves.clone(), hit_503.clone(), in_flight.clone());
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut line = String::new();
                    let _ = r.read_line(&mut line).await;
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    loop {
                        let mut h = String::new();
                        if r.read_line(&mut h).await.unwrap_or(0) == 0 || h.trim().is_empty() {
                            break;
                        }
                    }
                    if path == "/vod.m3u8" {
                        let mut m3u8 = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:5\n");
                        for i in 0..12 {
                            m3u8.push_str(&format!("#EXTINF:5.0,\n/seg-{i}.ts\n"));
                        }
                        m3u8.push_str("#EXT-X-ENDLIST\n");
                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", m3u8.len(), m3u8);
                        let _ = w.write_all(resp.as_bytes()).await;
                        return;
                    }
                    if let Some(idx) = path.strip_prefix("/seg-").and_then(|s| s.strip_suffix(".ts")).and_then(|n| n.parse::<usize>().ok()) {
                        let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        if cur > 2 {
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                            hit_503.fetch_add(1, Ordering::SeqCst);
                            let _ = w.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                            return;
                        }
                        serves[idx].fetch_add(1, Ordering::SeqCst);
                        let body = &parts_data[idx];
                        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = w.write_all(head.as_bytes()).await;
                        for chunk in body.chunks(8000) {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                            if w.write_all(chunk).await.is_err() {
                                break;
                            }
                        }
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        });
    }
    let dir = std::env::temp_dir().join(format!("aknan-hls-503-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let job = swiftget_engine::hls::HlsJob {
        url: format!("http://127.0.0.2:{port}/vod.m3u8"),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        output: dir.join("out.ts"),
        temp_dir: dir.join("parts"),
        connections: 10,
        retries: 1,
        limiter: Arc::new(SpeedLimiter::new(0)),
        clip: None,
    };
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));

    // Phase 1: run until >= 4 parts done despite 503 throttling, verify live estimate, then pause.
    let p1 = Arc::new(Progress::default());
    let c1 = CancellationToken::new();
    let h1 = tokio::spawn({
        let (s, j, p, c) = (shared.clone(), job.clone(), p1.clone(), c1.clone());
        async move { swiftget_engine::hls::run(s, j, p, c).await }
    });
    for _ in 0..200 {
        if p1.parts_done.load(Ordering::Relaxed) >= 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(p1.parts_done.load(Ordering::Relaxed) >= 4);
    assert!(p1.est_total.load(Ordering::Relaxed) > 0, "estimated total should be populated mid-stream");
    assert!(p1.bytes.load(Ordering::Relaxed) > 0);
    c1.cancel();
    assert!(h1.await.unwrap().is_err());
    assert!(hit_503.load(Ordering::SeqCst) > 0, "server 503 throttling path must be exercised");

    // Phase 2: resume from same temp_dir; finished parts must not be fetched again.
    let saved_parts = std::fs::read_dir(&job.temp_dir).unwrap().count();
    assert!(saved_parts >= 4);
    let p2 = Arc::new(Progress::default());
    swiftget_engine::hls::run(shared, job.clone(), p2.clone(), CancellationToken::new()).await.unwrap();
    let got = std::fs::read(&job.output).unwrap();
    let expected: Vec<u8> = parts_data.iter().flat_map(|v| v.iter().copied()).collect();
    assert!(got == expected, "resumed output mismatch: {} vs {}", got.len(), expected.len());
    assert!(serves.iter().filter(|c| c.load(Ordering::SeqCst) == 1).count() >= 10);
    assert!(swiftget_engine::http_download::learned_limit(&job.url).is_some_and(|n| n <= 2));
    let _ = std::fs::remove_dir_all(&dir);
}
