use crate::helpers::{ffmpeg, folder_site, make_video, wait_done};
use swiftget_engine::clip;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parts_of_a_stream_and_of_separate_picture_and_sound() {
    let Some(ff) = ffmpeg() else { return };
    let root = std::env::temp_dir().join(format!("aknan-clip-mgr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let site_dir = root.join("site");
    std::fs::create_dir_all(&site_dir).unwrap();
    let video = make_video(&site_dir);
    let run = |args: &[&str]| {
        let ok = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().unwrap();
        assert!(ok.success());
    };
    let v = video.to_str().unwrap();
    std::fs::create_dir_all(site_dir.join("hls")).unwrap();
    let list = site_dir.join("hls/list.m3u8");
    run(&["-i", v, "-c", "copy", "-f", "hls", "-hls_time", "4", "-hls_playlist_type", "vod", list.to_str().unwrap()]);
    run(&["-i", v, "-an", "-c", "copy", site_dir.join("v.mp4").to_str().unwrap()]);
    run(&["-i", v, "-vn", "-c", "copy", site_dir.join("a.m4a").to_str().unwrap()]);
    let port = folder_site(site_dir.clone()).await;
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let out = root.join("out");

    // a stream: only the parts that play between 50 s and 56 s
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/hls/list.m3u8"),
        file_name: Some("Stream.mp4".into()),
        save_dir: Some(out.clone()),
        clip: swiftget_engine::Clip::new(50.0, 56.0, true),
        start: true,
        ..Default::default()
    });
    let done = wait_done(&mgr, id).await;
    assert_eq!(done.status, swiftget_engine::Status::Completed, "{:?}", done.error);
    assert_eq!(done.file_name, "Stream [00-50 → 00-56].mp4");
    assert!(done.parts_total <= 3, "only the parts of the clip: {}", done.parts_total);
    let (len, pic) = clip::look(&ff, done.save_path.to_str().unwrap()).await.unwrap();
    assert!((len.unwrap() - 6.0).abs() < 0.2, "{len:?}");
    assert!(pic.is_some());

    // picture and sound apart (video page style), quick cut
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/v.mp4"),
        audio_url: Some(format!("http://127.0.0.1:{port}/a.m4a")),
        file_name: Some("Video.webm".into()),
        save_dir: Some(out.clone()),
        clip: swiftget_engine::Clip::new(30.0, 40.0, false),
        start: true,
        ..Default::default()
    });
    let done = wait_done(&mgr, id).await;
    assert_eq!(done.status, swiftget_engine::Status::Completed, "{:?}", done.error);
    assert_eq!(done.file_name, "Video [00-30 → 00-40].mp4");
    let said = std::process::Command::new("ffmpeg").arg("-i").arg(&done.save_path).output().unwrap();
    let said = String::from_utf8_lossy(&said.stderr);
    assert!(said.contains("Video:") && said.contains("Audio:"), "{said}");
    let len = clip::parse_duration(&said).unwrap();
    assert!((9.9..=12.1).contains(&len), "{len}");
    let _ = std::fs::remove_dir_all(&root);
}
