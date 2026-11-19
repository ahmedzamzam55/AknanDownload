use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use swiftget_engine::hls::HlsContainer;
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// End-to-end Wave 4: UTF-8 BOM master playlist, `#EXT-X-MEDIA:TYPE=AUDIO`
/// separate audio playlist, `data:` AES-128 key, Fake PNG wrapper before `0x47`
/// TS sync bytes, `.joined` marker & preserved `temp_dir`, plus 503 Retry-After.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_bom_fake_png_data_key_audio_and_preserved_temp() {
    let mut ts_seg = vec![0x11u8; 188 * 4];
    for off in [0usize, 188, 376, 564] {
        ts_seg[off] = 0x47;
    }
    let mut wrapped_ts = b"\x89PNG\r\n\x1a\nFAKEPNG".to_vec();
    wrapped_ts.extend_from_slice(&ts_seg);
    let mut aud_ts = vec![0x22u8; 188 * 2];
    aud_ts[0] = 0x47;
    aud_ts[188] = 0x47;

    // AES-128 PKCS7 encrypt `wrapped_ts` with 16-byte zero key + zero IV (seq 0)
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    let mut enc_buf = wrapped_ts.clone();
    enc_buf.resize(wrapped_ts.len() + 16, 0);
    let ct_len = cbc::Encryptor::<Aes128>::new_from_slices(&[0u8; 16], &[0u8; 16])
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut enc_buf, wrapped_ts.len())
        .unwrap()
        .len();
    enc_buf.truncate(ct_len);
    let enc_arc = Arc::new(enc_buf);
    let aud_arc = Arc::new(aud_ts);
    let hits = Arc::new(AtomicUsize::new(0));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let (enc_arc, aud_arc, hits) = (enc_arc.clone(), aud_arc.clone(), hits.clone());
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let (enc_arc, aud_arc, hits) = (enc_arc.clone(), aud_arc.clone(), hits.clone());
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
                    let reply = |status: &str, body: &[u8], extra: &str| {
                        let mut v = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", body.len()).into_bytes();
                        v.extend_from_slice(body);
                        v
                    };
                    let out = match path.as_str() {
                        "/master.m3u8" => reply(
                            "200 OK",
                            "\u{feff}#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio.m3u8\",DEFAULT=YES\n#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=1280x720,AUDIO=\"a\"\nvideo.m3u8\n".as_bytes(),
                            "",
                        ),
                        "/video.m3u8" => reply(
                            "200 OK",
                            "\u{feff}#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAA==\",IV=0x00000000000000000000000000000000\n#EXTINF:4,\nseg.png\n#EXT-X-ENDLIST\n".as_bytes(),
                            "",
                        ),
                        "/audio.m3u8" => reply("200 OK", b"#EXTM3U\n#EXTINF:4,\naud.ts\n#EXT-X-ENDLIST\n", ""),
                        "/seg.png" => {
                            if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                                reply("503 Service Unavailable", b"", "Retry-After: 1\r\n")
                            } else {
                                reply("200 OK", &enc_arc, "")
                            }
                        }
                        "/aud.ts" => reply("200 OK", &aud_arc, ""),
                        _ => reply("404 Not Found", b"", ""),
                    };
                    let _ = w.write_all(&out).await;
                });
            }
        });
    }

    let dir = std::env::temp_dir().join(format!("aknan-hls-overhaul-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let temp = dir.join("sgparts");
    let job = swiftget_engine::hls::HlsJob {
        url: format!("http://127.0.0.1:{port}/master.m3u8"),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        output: dir.join("video.joined"),
        temp_dir: temp.clone(),
        connections: 2,
        retries: 2,
        limiter: Arc::new(SpeedLimiter::new(0)),
        clip: None,
        initial_playlist: None,
    };
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let out = swiftget_engine::hls::run(shared, job, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert_eq!(out.container, HlsContainer::MpegTs);
    assert_eq!(std::fs::read(dir.join("video.joined")).unwrap(), ts_seg);
    assert_eq!(std::fs::read(out.audio_output.as_ref().unwrap()).unwrap(), *aud_arc);
    assert!(temp.join(".joined").is_file(), "temp_dir and .joined must remain until caller finish_file cleans up");
    assert!(temp.join(".meta.json").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
