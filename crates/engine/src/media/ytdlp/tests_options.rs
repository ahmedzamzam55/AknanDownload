use super::options::post_title;
use super::resolve::drive_original;
use super::*;
use crate::model::RequestHeaders;
use serde_json::{json, Value};

pub(super) fn youtube_sample() -> Value {
    let h = json!({"User-Agent": "Mozilla/5.0 test", "Accept": "*/*", "Sec-Fetch-Mode": "navigate"});
    json!({
        "title": "Cool video",
        "extractor_key": "Youtube",
        "formats": [
            {"format_id": "sb0", "protocol": "mhtml", "ext": "mhtml", "vcodec": "none", "acodec": "none", "url": "https://i.ytimg.com/sb"},
            {"format_id": "139", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.5", "abr": 48.8, "filesize": 1000, "url": "https://gv/139", "http_headers": h},
            {"format_id": "140", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.5, "filesize": 4000, "url": "https://gv/140", "http_headers": h},
            {"format_id": "251", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 135.0, "filesize": 4200, "url": "https://gv/251", "http_headers": h},
            {"format_id": "18", "protocol": "https", "ext": "mp4", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "fps": 25, "filesize_approx": 9000.4, "url": "https://gv/18", "http_headers": h},
            {"format_id": "136", "protocol": "https", "ext": "mp4", "vcodec": "avc1.4d401f", "acodec": "none", "height": 720, "fps": 25, "filesize": 50000, "url": "https://gv/136", "http_headers": h},
            {"format_id": "399", "protocol": "https", "ext": "mp4", "vcodec": "av01.0.08M.08", "acodec": "none", "height": 1080, "fps": 25, "filesize": 70000, "url": "https://gv/399", "http_headers": h},
            {"format_id": "137", "protocol": "https", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "fps": 25, "filesize": 90000, "url": "https://gv/137", "http_headers": h},
            {"format_id": "248", "protocol": "https", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "fps": 25, "filesize": 80000, "url": "https://gv/248", "http_headers": h},
            {"format_id": "303", "protocol": "https", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "fps": 60, "filesize": 120000, "url": "https://gv/303", "http_headers": h},
            {"format_id": "drm", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 2160, "has_drm": true, "url": "https://gv/drm"}
        ]
    })
}

#[test]
fn original_sound_track_wins_over_dubbed() {
    let info = serde_json::json!({
        "title": "t", "extractor_key": "Youtube",
        "formats": [
            {"format_id": "140-0", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 130.0,
             "language": "en", "language_preference": -1, "format_note": "English (United States) - dubbed-auto, medium", "url": "https://gv/en"},
            {"format_id": "140-1", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.0,
             "language": "ar", "language_preference": 10, "format_note": "Arabic original (default), medium", "url": "https://gv/ar"},
            {"format_id": "137", "protocol": "https", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "url": "https://gv/137"}
        ]
    });
    let r = build_options(&info);
    let best = r.options.iter().find(|o| o.key.starts_with("1080p")).unwrap();
    assert_eq!(best.audio_url.as_deref(), Some("https://gv/ar"));
    let audio = r.options.iter().find(|o| o.key.starts_with("audio|")).unwrap();
    assert_eq!(audio.video_url, "https://gv/ar");
}

#[test]
fn youtube_choices() {
    let info = build_options(&youtube_sample());
    assert_eq!(info.title, "Cool video");
    assert_eq!(info.site, "Youtube");
    let keys: Vec<&str> = info.options.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["1080p60|WEBM", "1080p|MP4", "1080p|WEBM", "1080p|MP4 AV1", "720p|MP4", "360p|MP4+A", "audio|M4A", "audio|MP3"]);
    let mp4 = &info.options[1];
    assert_eq!(mp4.video_url, "https://gv/137");
    assert_eq!(mp4.audio_url.as_deref(), Some("https://gv/140"));
    assert_eq!(mp4.size, 94000);
    assert_eq!(info.options[0].audio_url.as_deref(), Some("https://gv/251"));
    // AV1: its own choice, smaller than the H.264 one
    let av1 = &info.options[3];
    assert_eq!(av1.codec, "AV1");
    assert!(av1.label.ends_with("MP4 AV1"), "{}", av1.label);
    assert!(av1.size < mp4.size);
    assert_eq!(info.options[6].label, "Audio only · M4A 130k");
    assert_eq!(info.options[7].ext, "mp3");
    assert_eq!(info.options[7].video_url, info.options[6].video_url);
    // the resolution in plain words
    assert_eq!(mp4.label, "1080p Full HD · MP4");
    assert_eq!(mp4.tag, "Full HD");
    assert_eq!(info.options[4].tag, "HD");
    assert_eq!(info.options[5].tag, "SD");
    // a playlist asking for 1080: the H.264 one (plays everywhere)
    assert_eq!(pick_option(&info.options, "1080").unwrap().key, "1080p|MP4");
    assert_eq!(pick_option(&info.options, "mp3").unwrap().key, MP3_KEY);
    assert_eq!(pick_option(&info.options, "audio").unwrap().key, "audio|M4A");
}

#[test]
fn plain_words_for_resolutions() {
    assert_eq!(quality_tag(4320), "8K");
    assert_eq!(quality_tag(2160), "4K");
    assert_eq!(quality_tag(1440), "2K");
    assert_eq!(quality_tag(1080), "Full HD");
    assert_eq!(quality_tag(720), "HD");
    assert_eq!(quality_tag(480), "SD");
    assert_eq!(quality_tag(0), "");
}

#[test]
fn other_sites_without_codecs() {
    // TikTok / Instagram style: codecs not reported, video has sound
    let j = json!({
        "title": "Funny clip",
        "extractor_key": "TikTok",
        "formats": [
            {"format_id": "h264_540p", "protocol": "https", "ext": "mp4", "height": 1024, "width": 576, "filesize": 3000, "url": "https://v16/a.mp4", "http_headers": {"Referer": "https://www.tiktok.com/"}},
            {"format_id": "download", "protocol": "https", "ext": "mp4", "format_note": "watermarked", "url": "https://v16/b.mp4"},
            {"format_id": "hls-720", "protocol": "m3u8_native", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "mp4a", "url": "https://cdn/x.m3u8"}
        ]
    });
    let info = build_options(&j);
    let keys: Vec<&str> = info.options.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["1024p|MP4+A", "720p|MP4+A", "WATERMARKED|MP4+A"]);
    assert_eq!(info.options[0].audio_url, None);
    assert_eq!(info.options[1].ext, "ts");
    let mut h = RequestHeaders::default();
    apply_headers(&mut h, &info.options[0].headers);
    assert_eq!(h.referer.as_deref(), Some("https://www.tiktok.com/"));
}

#[test]
fn single_file_and_playlist() {
    let single = json!({"title": "Clip", "url": "https://x/v.mp4", "ext": "mp4", "protocol": "https"});
    assert_eq!(build_options(&single).options.len(), 1);
    let post = json!({"_type": "playlist", "title": "Post", "entries": [
        {"title": "Video 1", "formats": [{"url": "https://x/1.mp4", "ext": "mp4", "protocol": "https", "height": 640}]}
    ]});
    let info = build_options(&post);
    assert_eq!(info.title, "Video 1");
    assert_eq!(info.options[0].video_url, "https://x/1.mp4");
}

#[test]
fn drive_files_are_known() {
    assert_eq!(
        drive_file_id("https://drive.google.com/file/d/1aLKUiTGVrngA6MeznJwaivpiMOkOAm8D/view?usp=drive_link").as_deref(),
        Some("1aLKUiTGVrngA6MeznJwaivpiMOkOAm8D")
    );
    assert_eq!(drive_file_id("https://drive.google.com/open?id=1aLKUiTGVrngA6Mez").as_deref(), Some("1aLKUiTGVrngA6Mez"));
    assert_eq!(drive_file_id("https://drive.google.com/drive/my-drive"), None);
    assert_eq!(drive_file_id("https://example.com/file/d/1aLKUiTGVrngA6MeznJ/view"), None);
    let o = drive_original("1aLKUiTGVrngA6Mez", &YtInfo { title: String::new(), site: String::new(), options: Vec::new(), subtitles: Vec::new() });
    assert!(o.video_url.starts_with("https://drive.usercontent.google.com/download?id=1aLKUiTGVrngA6Mez"));
}

#[test]
fn post_titles_lose_their_counts() {
    assert_eq!(post_title("995K views · 9.9K reactions | It is real"), "It is real");
    assert_eq!(post_title("1.2M views | فيديو"), "فيديو");
    assert_eq!(post_title("Song A | Live"), "Song A | Live");
    assert_eq!(post_title("plain"), "plain");
}


#[test]
fn headers_are_applied() {
    let mut h = RequestHeaders::default();
    apply_headers(&mut h, &build_options(&youtube_sample()).options[0].headers);
    assert_eq!(h.user_agent.as_deref(), Some("Mozilla/5.0 test"));
    assert!(h.extra.iter().any(|(k, _)| k == "Sec-Fetch-Mode"));
    assert!(!h.extra.iter().any(|(k, _)| k == "Accept"));
}

#[test]
fn subtitle_tracks_sorted_and_curated() {
    let j = json!({
        "title": "Demo",
        "duration": 2400,
        "formats": [{"url": "https://cdn/v.m3u8", "protocol": "m3u8_native", "ext": "mp4", "height": 532, "tbr": 1145.0, "vcodec": "avc1", "acodec": "mp4a"}],
        "subtitles": {
            "en": [{"url": "https://cdn/en.vtt", "ext": "vtt"}],
            "ar": [{"url": "https://cdn/ar.srt", "ext": "srt"}, {"url": "https://cdn/ar.vtt", "ext": "vtt"}]
        },
        "automatic_captions": {
            "fr": [{"url": "https://cdn/fr.json3?tlang=fr", "ext": "json3"}],
            "en": [{"url": "https://cdn/auto-en.vtt", "ext": "vtt"}],
            "xx-ignored": [{"url": "https://cdn/xx.vtt?tlang=xx", "ext": "vtt"}]
        }
    });
    let info = build_options(&j);
    assert!(info.options[0].size > 300_000_000, "HLS bitrate * duration estimates MB size: {}", info.options[0].size);
    let langs: Vec<&str> = info.subtitles.iter().map(|s| s.lang.as_str()).collect();
    assert_eq!(langs, ["ar", "en", "fr", "xx-ignored"]);
    assert_eq!(info.subtitles[0].ext, "srt");
    assert!(!info.subtitles[0].auto);
    assert!(info.subtitles[2].auto);
}

#[test]
fn hls_split_pairs_video_only_m3u8_with_audio() {
    let j = json!({
        "title": "Kids Song",
        "duration": 120,
        "formats": [
            {"format_id": "232", "protocol": "m3u8_native", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 720, "tbr": 1500.0, "url": "https://cdn/v720.m3u8"},
            {"format_id": "234", "protocol": "m3u8_native", "ext": "mp4", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128.0, "url": "https://cdn/a128.m3u8"}
        ]
    });
    let info = build_options(&j);
    let v = info.options.iter().find(|o| o.quality == "720p").expect("720p split HLS paired");
    assert_eq!(v.video_url, "https://cdn/v720.m3u8");
    assert_eq!(v.audio_url.as_deref(), Some("https://cdn/a128.m3u8"));
    assert!(v.size > 0);
}

