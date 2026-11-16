use super::*;

#[test]
fn preview_of_half_downloaded_video_and_sound() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("swiftget-preview-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let (v, a) = (dir.join(".x.video.sgpart"), dir.join(".x.audio.sgpart"));
    let gen = |args: &[&str], out: &Path| {
        let ok = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).arg(out).status().unwrap();
        assert!(ok.success());
    };
    // like YouTube: fragmented MP4, picture and sound apart
    gen(&["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=20", "-c:v", "mpeg4", "-an", "-movflags", "+frag_keyframe+empty_moov", "-f", "mp4"], &v);
    gen(&["-f", "lavfi", "-i", "sine=frequency=440:duration=20", "-c:a", "aac", "-vn", "-movflags", "+frag_keyframe+empty_moov", "-f", "mp4"], &a);
    // the files are full size already (sparse), only half is real
    let (vl, al) = (std::fs::metadata(&v).unwrap().len(), std::fs::metadata(&a).unwrap().len());
    let out = dir.join("x (preview).mkv");
    preview_snapshot(&v, vl / 2, Some((&a, al / 2)), &out).unwrap();
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video: mpeg4") && info.contains("Audio: aac"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stream_to_mp4() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("swiftget-ts-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let ts = dir.join("x.ts.sgpart");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "mpeg4", "-c:a", "aac", "-f", "mpegts"])
        .arg(&ts)
        .status()
        .unwrap();
    assert!(ok.success());
    let out = dir.join("x.mp4.tmp");
    ts_to_mp4(&ts, &out, &|_| {}, None).unwrap();
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("mp4") && info.contains("Audio: aac"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn makes_mp3() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join("swiftget-mp3-test");
    let _ = std::fs::create_dir_all(&dir);
    let a = dir.join("a.m4a.sgpart");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac", "-f", "mp4"])
        .arg(&a)
        .status()
        .unwrap();
    assert!(ok.success());
    let out = dir.join("song.mp3.tmp");
    to_mp3(&a, &out, 192, Some("Song")).unwrap();
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Audio: mp3") && info.contains("Song"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Real merge, only when ffmpeg is installed (skipped otherwise).
#[test]
fn merges_video_and_audio() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join("swiftget-mux-test");
    let _ = std::fs::create_dir_all(&dir);
    let (v, a) = (dir.join("v.mp4"), dir.join("a.m4a"));
    let gen = |args: &[&str]| {
        let ok = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().unwrap();
        assert!(ok.success());
    };
    gen(&["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-c:v", "mpeg4", "-an", v.to_str().unwrap()]);
    gen(&["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac", "-vn", a.to_str().unwrap()]);
    // the output still has the .sgpart name while downloading
    let out = dir.join("Clip [1080p].mp4.sgpart");
    let last = std::cell::Cell::new(0.0);
    merge(&v, &a, &out, "Clip [1080p].mp4", &|f| last.set(f), None).unwrap();
    // the second bar reaches its end
    assert_eq!(last.get(), 1.0);
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video: mpeg4") && info.contains("Audio: aac"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn formats() {
    assert_eq!(format_for("a [1080p].mp4"), "mp4");
    assert_eq!(format_for("a.webm"), "webm");
    assert_eq!(format_for("a.mkv"), "matroska");
}

#[test]
fn burns_subtitles_into_mp4() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("swiftget-burn-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let v = dir.join("in.mp4");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=1", "-c:v", "mpeg4", "-an"])
        .arg(&v)
        .status()
        .unwrap();
    assert!(ok.success());
    let out = dir.join("out.mp4.sgpart");
    let srt = "1\n00:00:00,000 --> 00:00:00,900\nمرحبا\n\n";
    burn_subtitles(&v, srt, &out, "Clip.mp4", &|_| {}, None).unwrap();
    assert!(std::fs::metadata(&out).unwrap().len() > 1000);
    let _ = std::fs::remove_dir_all(&dir);
}
