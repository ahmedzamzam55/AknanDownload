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
