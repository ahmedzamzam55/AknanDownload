use crate::helpers::{ffmpeg, make_video, site};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use swiftget_engine::clip::{self, Output};
use swiftget_engine::clipserve::{self, Source};
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_part_is_read_and_cut_where_asked() {
    let Some(ff) = ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("aknan-clip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = make_video(&dir);
    let data = Arc::new(std::fs::read(&video).unwrap());
    let sent = Arc::new(AtomicU64::new(0));
    let port = site(data.clone(), sent.clone()).await;

    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let progress = Arc::new(Progress::default());
    let src = Source { url: format!("http://127.0.0.1:{port}/v.mp4"), headers: RequestHeaders::default(), user_agent: "test".into() };
    let ticket = clipserve::open(shared.clone(), src, progress.clone()).await.unwrap();

    // what the video is
    let (len, pic) = clip::look(&ff, &ticket.url).await.unwrap();
    assert!((len.unwrap() - 120.0).abs() < 0.2, "{len:?}");
    assert_eq!(pic.unwrap().height, 360);

    // the quick cut: 3 s from 21.0 s
    sent.store(0, Ordering::Relaxed);
    let quick = dir.join("quick.mp4");
    let cancel = CancellationToken::new();
    let last = Arc::new(std::sync::Mutex::new(0.0));
    let l2 = last.clone();
    let done = clip::cut(&ff, std::slice::from_ref(&ticket.url), 21.0, 3.0, false, &Output::Video, &quick, move |f| *l2.lock().unwrap() = f, &cancel)
        .await
        .unwrap();
    assert_eq!(done, clip::Done::AsAsked);
    let (qlen, _) = clip::look(&ff, quick.to_str().unwrap()).await.unwrap();
    let qlen = qlen.unwrap();
    // starts on the whole picture at 20 s: 3 s, or up to 2 s more
    assert!((2.9..=5.2).contains(&qlen), "quick part lasts {qlen}");
    let read = sent.load(Ordering::Relaxed);
    assert!(read < data.len() as u64 / 3, "only the part was read: {read} of {} bytes", data.len());
    assert!(*last.lock().unwrap() > 0.5, "progress was told");

    // the exact cut: exactly 3 s
    let exact = dir.join("exact.mp4");
    let done = clip::cut(&ff, std::slice::from_ref(&ticket.url), 21.0, 3.0, true, &Output::Video, &exact, |_| {}, &cancel).await.unwrap();
    assert_eq!(done, clip::Done::AsAsked);
    let (elen, _) = clip::look(&ff, exact.to_str().unwrap()).await.unwrap();
    assert!((elen.unwrap() - 3.0).abs() < 0.15, "exact part lasts {elen:?}");

    // sound alone, as MP3
    let mp3 = dir.join("part.mp3");
    clip::cut(&ff, std::slice::from_ref(&ticket.url), 5.0, 4.0, false, &Output::Mp3(128, Some("Song".into())), &mp3, |_| {}, &cancel)
        .await
        .unwrap();
    let (mlen, mpic) = clip::look(&ff, mp3.to_str().unwrap()).await.unwrap();
    assert!((mlen.unwrap() - 4.0).abs() < 0.15, "mp3 part lasts {mlen:?}");
    assert!(mpic.is_none());

    // a ticket that went: the address stops working
    let url = ticket.url.clone();
    drop(ticket);
    assert!(clip::look(&ff, &url).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_stops_ffmpeg() {
    let Some(ff) = ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("aknan-clip-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = make_video(&dir);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        c2.cancel();
    });
    let t = std::time::Instant::now();
    let r = clip::cut(&ff, &[video.to_string_lossy().into_owned()], 0.0, 120.0, true, &Output::Video, &dir.join("x.mp4"), |_| {}, &cancel).await;
    assert!(r.is_err() && r.unwrap_err().is::<swiftget_engine::http_download::Cancelled>());
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
}
