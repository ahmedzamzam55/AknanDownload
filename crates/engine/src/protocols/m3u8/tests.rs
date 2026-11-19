use super::*;

#[test]
fn only_the_parts_of_a_clip() {
    let t = "#EXTM3U\n#EXTINF:4.0,\na.ts\n#EXTINF:4.0,\nb.ts\n#EXTINF:4.0,\nc.ts\n#EXTINF:4.0,\nd.ts\n#EXT-X-ENDLIST\n";
    let pl = parse_media("https://x.test/v/list.m3u8", t).unwrap();
    assert_eq!(pl.duration(), 16.0);
    assert_eq!(pl.parts_between(5.0, 9.0), (vec![1, 2], 4.0));
    assert_eq!(pl.parts_between(0.0, 4.0), (vec![0], 0.0));
    assert_eq!(pl.parts_between(15.0, 30.0), (vec![3], 12.0));
}

#[test]
fn oversized_lists_are_refused() {
    let mut huge = String::from("#EXTM3U\n");
    for i in 0..=MAX_PARTS {
        huge.push_str(&format!("p{i}.ts\n"));
    }
    assert!(parse_media("https://x.example/a/list.m3u8", &huge).is_err());
    assert!(parse_media("https://x.example/a/list.m3u8", &"#".repeat(MAX_PLAYLIST_BYTES + 1)).is_err());
    assert!(parse_media("https://x.example/a/list.m3u8", "#EXTM3U\np1.ts\n").is_ok());
}

#[test]
fn byte_ranges() {
    let t = "#EXTM3U\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"720@0\"\n#EXTINF:6,\n#EXT-X-BYTERANGE:1000@720\nall.mp4\n#EXTINF:6,\n#EXT-X-BYTERANGE:500\nall.mp4\n#EXTINF:6,\nother.ts\n#EXT-X-ENDLIST\n";
    let p = parse_media("https://cdn.x/v/list.m3u8", t).unwrap();
    assert_eq!(p.init_url.as_deref(), Some("https://cdn.x/v/all.mp4"));
    assert_eq!(p.init_range, Some((0, 720)));
    assert_eq!(p.parts[0].range, Some((720, 1000)));
    assert_eq!(p.parts[1].range, Some((1720, 500)));
    assert_eq!(p.parts[2].range, None);
}

#[test]
fn attrs_with_quotes() {
    let a = parse_attrs("METHOD=AES-128,URI=\"https://k/key?a=1,2\",IV=0x00000000000000000000000000000001");
    assert_eq!(a["METHOD"], "AES-128");
    assert_eq!(a["URI"], "https://k/key?a=1,2");
    assert_eq!(parse_iv(&a["IV"]).unwrap()[15], 1);
}

#[test]
fn master_picks_highest_bandwidth() {
    let m = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhigh/index.m3u8\n";
    let u = best_variant("https://cdn.x/v/master.m3u8", m).unwrap().unwrap();
    assert_eq!(u, "https://cdn.x/v/high/index.m3u8");
}

#[test]
fn media_playlist() {
    let m = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n#EXT-X-ENDLIST\n";
    let p = parse_media("https://cdn.x/v/index.m3u8", m).unwrap();
    assert!(p.ended);
    assert_eq!(p.parts.len(), 2);
    assert_eq!(p.parts[1].url, "https://cdn.x/v/b.ts");
    assert_eq!(p.parts[1].sequence, 8);
    assert_eq!(p.parts[0].key.as_ref().unwrap().uri, "https://cdn.x/v/key.bin");
}

#[test]
fn drm_is_refused() {
    let m = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\na.ts\n#EXT-X-ENDLIST\n";
    assert!(parse_media("https://cdn.x/index.m3u8", m).is_err());
}

#[test]
fn bom_audio_group_discontinuity_and_multi_key_work() {
    let master = "\u{feff}#EXTM3U\n#EXT-X-MEDIA:LANGUAGE=\"ar\",TYPE=AUDIO,GROUP-ID=\"aud\",URI=\"a.m3u8\",DEFAULT=YES\n#EXT-X-STREAM-INF:BANDWIDTH=1200000,RESOLUTION=1280x720,AUDIO=\"aud\"\nv.m3u8\n";
    let (vars, sep) = parse_master("https://cdn.x/m.m3u8", master).unwrap();
    assert!(sep);
    assert_eq!(vars[0].audio_url.as_deref(), Some("https://cdn.x/a.m3u8"));

    let media = "\u{feff}#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"data:text/plain;base64,AQ==\"\n#EXT-X-MAP:URI=\"init1.mp4\"\n#EXTINF:4,\n1.m4s\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init2.mp4\"\n#EXTINF:4,\n2.m4s\n#EXT-X-ENDLIST\n";
    let pl = parse_media("https://cdn.x/v/list.m3u8", media).unwrap();
    assert_eq!(pl.parts.len(), 2);
    assert_eq!(pl.parts[0].init_url.as_deref(), Some("https://cdn.x/v/init1.mp4"));
    assert!(!pl.parts[0].discontinuity);
    assert!(pl.parts[1].discontinuity);
    assert_eq!(pl.parts[1].init_url.as_deref(), Some("https://cdn.x/v/init2.mp4"));
}
