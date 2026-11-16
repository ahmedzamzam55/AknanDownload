use super::probe::video_kbps;
use super::*;


#[test]
fn what_ffmpeg_says() {
    let said = "Input #0, mov,mp4, from 'x':\n  Duration: 00:03:25.48, start: 0.000000, bitrate: 1200 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1280x720 [SAR 1:1 DAR 16:9], 1000 kb/s, 30 fps\n  Stream #0:1: Audio: aac";
    assert_eq!(parse_duration(said), Some(205.48));
    assert_eq!(parse_picture(said), Some(Picture { codec: "h264".into(), width: 1280, height: 720 }));
    assert!(parse_picture("Stream #0:0: Audio: opus").is_none());
    assert_eq!(parse_duration("Duration: N/A, bitrate"), None);
}

#[test]
fn quick_and_exact_words() {
    let ins = vec!["v".to_string(), "a".to_string()];
    let q = cut_args(&ins, 10.0, 5.0, None, &Output::Video).join(" ");
    assert!(q.contains("-ss 10.000 -i v -ss 10.000 -i a -t 5.000"));
    assert!(q.contains("-map 0:v:0 -map 1:a:0? -c copy"));
    let e = cut_args(&ins[..1], 10.0, 5.0, Some(("libopenh264", 4000)), &Output::Video).join(" ");
    assert!(e.contains("-c:v libopenh264 -b:v 4000k") && e.contains("-c:a aac"));
    let m = cut_args(&ins[..1], 1.0, 2.0, None, &Output::Mp3(192, Some("Song".into()))).join(" ");
    assert!(m.contains("-vn -map 0:a:0") && m.contains("libmp3lame -b:a 192k") && m.contains("title=Song"));
    assert_eq!(video_kbps(&Picture { codec: "h264".into(), width: 1920, height: 1080 }), 8000);
}
