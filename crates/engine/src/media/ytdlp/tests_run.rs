use super::cookies::{cookie_file, login_wall, needs_login, site_of};
use super::hls_direct::try_resolve_hls;
use super::playlist::{channel_videos, script_lang};
use super::subtitles::{playlist_args, resolve_args, resolve_args_with};
use super::tests_options::youtube_sample;
use super::*;
use serde_json::json;
use std::path::Path;

#[test]
fn names_tell_their_language() {
    assert_eq!(script_lang("القرآن كامل بصوت الشيخ"), Some("ar"));
    assert_eq!(script_lang("07 - Привет"), Some("ru"));
    assert_eq!(script_lang("Surah Al-Asr - سورة"), None);
    assert_eq!(script_lang("123"), None);
}

#[test]
fn channel_pages_list_their_videos() {
    assert_eq!(channel_videos("https://www.youtube.com/@name"), "https://www.youtube.com/@name/videos");
    assert_eq!(channel_videos("https://www.youtube.com/@name/"), "https://www.youtube.com/@name/videos");
    assert_eq!(channel_videos("https://www.youtube.com/@name/featured"), "https://www.youtube.com/@name/videos");
    assert_eq!(channel_videos("https://www.youtube.com/@name/shorts"), "https://www.youtube.com/@name/shorts");
    assert_eq!(channel_videos("https://www.youtube.com/playlist?list=PL1"), "https://www.youtube.com/playlist?list=PL1");
    let tabs = serde_json::json!({"_type": "playlist", "title": "Name", "entries": [
        {"_type": "url", "ie_key": "YoutubeTab", "url": "https://www.youtube.com/@name/videos", "title": "Name - Videos"},
        {"_type": "url", "ie_key": "YoutubeTab", "url": "https://www.youtube.com/@name/shorts", "title": "Name - Shorts"}
    ]});
    let p = parse_playlist(&tabs);
    assert!(p.entries.is_empty());
    assert_eq!(p.tabs.len(), 2);
    let a: Vec<String> = playlist_args("u", None, "ar").iter().map(|x| x.to_string_lossy().into_owned()).collect();
    assert!(a.windows(2).any(|w| w[0] == "--extractor-args" && w[1] == "youtube:lang=ar"));
    let b: Vec<String> = playlist_args("u", None, "a b").iter().map(|x| x.to_string_lossy().into_owned()).collect();
    assert!(!b.contains(&"--extractor-args".to_string()));
}

#[test]
fn playlists_and_quality_choice() {
    let j = json!({"_type": "playlist", "title": "Course", "entries": [
        {"id": "aaa", "title": "Lesson 1", "url": "https://www.youtube.com/watch?v=aaa", "duration": 61.0},
        {"id": "bbb", "title": "[Private video]", "url": "https://www.youtube.com/watch?v=bbb"},
        {"id": "ccc", "title": "Lesson 3"}
    ]});
    let p = parse_playlist(&j);
    assert_eq!(p.title, "Course");
    assert_eq!(p.entries.len(), 2);
    assert_eq!(p.entries[0].duration, 61);
    assert_eq!(p.entries[1].url, "https://www.youtube.com/watch?v=ccc");

    let opts = build_options(&youtube_sample()).options;
    assert_eq!(pick_option(&opts, "best").unwrap().key, "1080p|MP4");
    assert_eq!(pick_option(&opts, "1080").unwrap().key, "1080p|MP4");
    assert_eq!(pick_option(&opts, "720").unwrap().key, "720p|MP4");
    assert_eq!(pick_option(&opts, "480").unwrap().key, "360p|MP4+A");
    assert_eq!(pick_option(&opts, "240").unwrap().key, "360p|MP4+A");
    assert_eq!(pick_option(&opts, "audio").unwrap().key, "audio|M4A");
}


#[test]
fn yt_dlp_settings_files_and_options_are_never_used() {
    let a: Vec<String> = resolve_args("-x --exec calc", Some(Path::new("c.txt")), Some("-o x")).iter().map(|x| x.to_string_lossy().into_owned()).collect();
    assert_eq!(a[0], "--ignore-config");
    // the address comes after "--": yt-dlp can't read it as an option
    assert_eq!(&a[a.len() - 2..], ["--", "-x --exec calc"]);
    assert!(!a.iter().any(|x| x == "--referer"), "only a web address is a referer");
    let r: Vec<String> = resolve_args("https://cdn/x.mpd", None, Some("https://site/page")).iter().map(|x| x.to_string_lossy().into_owned()).collect();
    assert!(r.windows(2).any(|w| w[0] == "--referer" && w[1] == "https://site/page"));
    let p: Vec<String> = playlist_args("-o x", None, "").iter().map(|x| x.to_string_lossy().into_owned()).collect();
    assert_eq!(p[0], "--ignore-config");
    assert_eq!(&p[p.len() - 2..], ["--", "-o x"]);
}

#[test]
fn cookies_are_kept_per_site() {
    assert_eq!(site_of("https://www.youtube.com/watch?v=x"), "youtube.com");
    assert_eq!(site_of("https://youtu.be/x"), "youtube.com");
    assert_eq!(site_of("https://m.youtube.com/watch?v=x"), "youtube.com");
    assert_eq!(site_of("https://www.bbc.co.uk/x"), "bbc.co.uk");
    let c = BrowserCookie {
        domain: ".youtube.com".into(),
        name: "SID".into(),
        value: "v".into(),
        path: "/".into(),
        secure: true,
        expiration_date: None,
    };
    remember_cookies("https://www.youtube.com/watch?v=a", &[c]);
    assert_eq!(remembered_cookies("https://youtu.be/b").unwrap()[0].name, "SID");
    assert!(remembered_cookies("https://vimeo.com/1").is_none());
    assert!(login_wall("Sign in to confirm your age. This video may be inappropriate for some users."));
    assert!(!login_wall("Unsupported URL: https://example.com/page"));
}

#[test]
fn cookie_files_are_new_every_time() {
    let list = vec![BrowserCookie {
        domain: ".example.com".into(),
        name: "a".into(),
        value: "b".into(),
        path: "/".into(),
        secure: true,
        expiration_date: None,
    }];
    let (a, b) = (cookie_file(&list).unwrap(), cookie_file(&list).unwrap());
    assert_ne!(a, b);
    assert!(std::fs::read_to_string(&a).unwrap().contains("example.com"));
    let _ = std::fs::remove_file(a);
    let _ = std::fs::remove_file(b);
}

#[test]
fn cookie_file_format() {
    let list = vec![
        BrowserCookie { domain: ".instagram.com".into(), name: "sessionid".into(), value: "abc".into(), path: "/".into(), secure: true, expiration_date: Some(1900000000.5) },
        BrowserCookie { domain: "www.x.com".into(), name: "a".into(), value: "b=c".into(), path: "".into(), secure: false, expiration_date: None },
    ];
    let s = netscape_cookies(&list);
    assert!(s.starts_with("# Netscape HTTP Cookie File\n"));
    assert!(s.contains(".instagram.com\tTRUE\t/\tTRUE\t1900000000\tsessionid\tabc\n"));
    assert!(s.contains("www.x.com\tFALSE\t/\tFALSE\t2147483647\ta\tb=c\n"));
    assert!(needs_login("ERROR: [Instagram] abc: Requested content is not available, rate-limit reached or login required"));
}

#[test]
fn extra_headers_forwarded_to_ytdlp_and_direct_hls() {
    let args: Vec<String> = resolve_args_with(
        "https://cdn.example/v.mp4",
        None,
        Some("https://embed.example/p"),
        &[("Origin".into(), "https://embed.example".into()), ("User-Agent".into(), "CustomUA".into())],
    )
    .iter()
    .map(|x| x.to_string_lossy().into_owned())
    .collect();
    assert!(args.windows(2).any(|w| w[0] == "--user-agent" && w[1] == "CustomUA"));
    assert!(args.windows(2).any(|w| w[0] == "--add-header" && w[1] == "Origin:https://embed.example"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            let ok = req.contains("referer: https://embed.example/v1") && req.contains("origin: https://embed.example");
            let resp = if ok {
                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n\
                 #EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n720/index.m3u8\n"
            } else {
                "HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n"
            };
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    let info = try_resolve_hls(
        &format!("http://{addr}/hls2/master.m3u8?t=tok"),
        None,
        Some("https://outer.site/watch"),
        &[
            ("Referer".into(), "https://embed.example/v1".into()),
            ("Origin".into(), "https://embed.example".into()),
        ],
    )
    .unwrap()
    .unwrap();
    let _ = t.join();
    assert_eq!(info.options.len(), 1);
    assert_eq!(info.options[0].quality, "720p");
    assert_eq!(info.options[0].video_url, format!("http://{addr}/hls2/720/index.m3u8"));
    assert!(info.options[0].headers.iter().any(|(k, v)| k == "Origin" && v == "https://embed.example"));
    assert!(info.options[0].headers.iter().any(|(k, v)| k == "Referer" && v == "https://embed.example/v1"));
}

#[test]
fn hls_direct_computes_duration_and_byte_size_from_variant() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = std::thread::spawn(move || {
        use std::io::{Read, Write};
        for s in listener.incoming().flatten().take(2) {
            let mut sock = s;
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let first = req.lines().next().unwrap_or("");
            let body = if first.contains("/master.m3u8") {
                "#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Arabic\",LANGUAGE=\"ar\",URI=\"ar.vtt\"\n#EXT-X-STREAM-INF:BANDWIDTH=1145000,RESOLUTION=960x532,SUBTITLES=\"subs\"\n532/index.m3u8\n"
            } else {
                "#EXTM3U\n#EXTINF:1200.0,\nseg0.ts\n#EXTINF:1200.0,\nseg1.ts\n#EXT-X-ENDLIST\n"
            };
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nConnection: close\r\n\r\n{body}");
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    let info = try_resolve_hls(&format!("http://{addr}/master.m3u8"), None, None, &[]).unwrap().unwrap();
    let _ = t.join();
    assert_eq!(info.options[0].duration, 2400);
    assert_eq!(info.options[0].size, 2400 * 1_145_000 / 8);
    assert_eq!(info.subtitles.len(), 1);
    assert_eq!(info.subtitles[0].lang, "ar");
}

