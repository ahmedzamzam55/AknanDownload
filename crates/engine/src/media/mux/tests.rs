use super::*;

#[test]
fn preview_of_half_downloaded_video_and_sound() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("swiftget-preview-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let v = make_test_mp4(&ffmpeg, &dir, ".x.video.sgpart", 8, &["-g", "12", "-movflags", "+frag_keyframe+empty_moov", "-frag_duration", "500000", "-f", "mp4"]);
    let a = make_test_m4a(&ffmpeg, &dir, ".x.audio.sgpart", 8, &["-movflags", "+frag_keyframe+empty_moov", "-frag_duration", "500000", "-f", "mp4"]);
    let (vl, al) = (std::fs::metadata(&v).unwrap().len(), std::fs::metadata(&a).unwrap().len());
    let out = dir.join("x (preview).mkv");
    preview_snapshot(&v, vl / 2, Some((&a, al / 2)), &out).unwrap();
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video: h264") && info.contains("Audio: aac"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stream_to_mp4() {
    let Some(ffmpeg) = find_ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("swiftget-ts-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let v = make_test_mp4(&ffmpeg, &dir, "v.mp4", 1, &[]);
    let a = make_test_m4a(&ffmpeg, &dir, "a.m4a", 1, &["-f", "mp4"]);
    let ts = dir.join("x.ts.sgpart");
    let ok = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&v)
        .arg("-i")
        .arg(&a)
        .args(["-c", "copy", "-f", "mpegts"])
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
    let dir = std::env::temp_dir().join(format!("swiftget-mp3-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let a = make_test_m4a(&ffmpeg, &dir, "a.m4a.sgpart", 2, &["-f", "mp4"]);
    let out = dir.join("song.mp3.tmp");
    to_mp3(&a, &out, 192, Some("Song")).unwrap();
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Audio: mp3") && info.contains("Song"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn make_test_mp4(ffmpeg: &Path, dir: &Path, name: &str, secs: usize, extra: &[&str]) -> std::path::PathBuf {
    let out = dir.join(name);
    let raw = dir.join(format!("{name}.rgb"));
    let frame_sz = 320 * 240 * 3;
    let frames = 25 * secs;
    let mut rgb = vec![0u8; frame_sz * frames];
    for f in 0..frames {
        let base = f * frame_sz;
        for i in 0..frame_sz {
            rgb[base + i] = ((i.wrapping_mul(13) + f.wrapping_mul(29)) & 0xff) as u8;
        }
    }
    std::fs::write(&raw, &rgb).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", "320x240", "-r", "25", "-i"])
        .arg(&raw)
        .args(["-c:v", crate::clip::h264_maker(ffmpeg), "-b:v", "800k", "-pix_fmt", "yuv420p"])
        .args(extra)
        .arg(&out)
        .status()
        .unwrap();
    let _ = std::fs::remove_file(&raw);
    assert!(ok.success(), "failed to generate test video with {}", ffmpeg.display());
    out
}

fn make_test_m4a(ffmpeg: &Path, dir: &Path, name: &str, secs: u32, extra: &[&str]) -> std::path::PathBuf {
    let wav = dir.join(format!("{name}.wav"));
    let out = dir.join(name);
    let data_bytes: u32 = 48_000 * 2 * secs;
    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&48_000u32.to_le_bytes());
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    buf.resize(44 + data_bytes as usize, 0);
    std::fs::write(&wav, &buf).unwrap();
    let ok = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&wav)
        .args(["-c:a", "aac", "-vn"])
        .args(extra)
        .arg(&out)
        .status()
        .unwrap();
    let _ = std::fs::remove_file(&wav);
    assert!(ok.success(), "failed to generate test audio with {}", ffmpeg.display());
    out
}

/// Real merge against bundled/PATH ffmpeg (fails if tools/ffmpeg/win64 exists yet doesn't resolve).
#[test]
fn merges_video_and_audio() {
    let local_ff = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ffmpeg/win64/ffmpeg.exe");
    let Some(ffmpeg) = find_ffmpeg() else {
        assert!(!local_ff.exists(), "tools/ffmpeg/win64/ffmpeg.exe exists yet find_ffmpeg() returned None");
        return;
    };
    let dir = std::env::temp_dir().join(format!("swiftget-mux-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let v = make_test_mp4(&ffmpeg, &dir, "v.mp4", 1, &[]);
    let a = make_test_m4a(&ffmpeg, &dir, "a.m4a", 1, &[]);
    let out = dir.join("Clip [1080p].mp4.sgpart");
    let last = std::cell::Cell::new(0.0);
    merge(&v, &a, &out, "Clip [1080p].mp4", &|f| last.set(f), None).unwrap();
    assert_eq!(last.get(), 1.0);
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video: h264") && info.contains("Audio: aac"), "{info}");
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
    let local_ff = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ffmpeg/win64/ffmpeg.exe");
    let Some(ffmpeg) = find_ffmpeg() else {
        assert!(!local_ff.exists(), "tools/ffmpeg/win64/ffmpeg.exe exists yet find_ffmpeg() returned None");
        return;
    };
    let dir = std::env::temp_dir().join(format!("swiftget-burn-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let v = make_test_mp4(&ffmpeg, &dir, "in.mp4", 1, &[]);
    let out = dir.join("out.mp4.sgpart");
    let srt = "1\n00:00:00,050 --> 00:00:00,950\nمرحباً بكم في أكنان للتحميل\n\n";
    burn_subtitles(&v, srt, &out, "Clip.mp4", &|_| {}, None).unwrap();
    assert!(std::fs::metadata(&out).unwrap().len() > 1000);
    let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video: h264"), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}
