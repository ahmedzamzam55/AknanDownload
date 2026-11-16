#[cfg(test)]
mod name_tests {
    use super::super::resolve::playlist_stem;

    #[test]
    fn playlist_videos_keep_the_name_of_the_list() {
        // the list showed it in the user's language: that name stays
        assert_eq!(playlist_stem("07 - طريقة عمل الأرز", "How to cook rice"), "07 - طريقة عمل الأرز");
        assert_eq!(playlist_stem("2024 plans", "Plans"), "2024 plans");
        assert_eq!(playlist_stem("07 - Old", "  "), "07 - Old");
        // no name in the list: the video's own
        assert_eq!(playlist_stem("07 - video", "طريقة عمل الأرز"), "07 - طريقة عمل الأرز");
        assert_eq!(playlist_stem("video", "Plans"), "Plans");
    }
}

#[cfg(test)]
mod inside_tests {
    use super::super::cleanup::inside;
    use super::super::clip_job::song_title;
    #[test]
    fn mp3_titles() {
        assert_eq!(song_title("My Song [mp3].mp3"), "My Song");
        assert_eq!(song_title("Live [2024] mix.mp3"), "Live [2024] mix");
        assert_eq!(song_title("plain.mp3"), "plain");
    }
    use std::path::Path;
    #[test]
    fn nothing_outside_the_folder() {
        let d = Path::new("/d/Video");
        assert!(inside(Path::new("/d/Video/pack/a.bin"), d));
        assert!(!inside(Path::new("/d/Video/../x.bin"), d));
        assert!(!inside(Path::new("/d/Video"), d));
        assert!(!inside(Path::new("/d/Other/a.bin"), d));
    }
}

#[cfg(test)]
mod media_tests {
    use super::super::finish::not_the_media;
    use super::super::post_http::{find_holes, is_packed, segments_for_holes};

    #[test]
    fn empty_parts_of_a_packed_file_are_found() {
        let dir = std::env::temp_dir().join(format!("aknan-holes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.rar.sgpart");
        let mut data: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8 | 1).collect();
        for b in &mut data[1_000_123..1_300_000] {
            *b = 0;
        }
        // a few zeros in a row are normal
        for b in &mut data[2_000_000..2_000_100] {
            *b = 0;
        }
        std::fs::write(&p, &data).unwrap();
        assert_eq!(find_holes(&p), vec![(1_000_123, 1_300_000)]);
        assert!(is_packed("big.part01.rar") && is_packed("x.ZIP") && !is_packed("disk.iso"));
        let t = segments_for_holes(3_000_000, &[(1_000_123, 1_300_000)]);
        assert_eq!(crate::segments::downloaded(&t), 3_000_000 - (1_300_000 - 1_000_123));
        assert_eq!(t.iter().map(|s| s.end - s.start).sum::<u64>(), 3_000_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_is_not_a_video() {
        let dir = std::env::temp_dir().join(format!("aknan-notmedia-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.sgpart");
        std::fs::write(&p, b"<!DOCTYPE html><html><body>no</body></html>").unwrap();
        assert!(not_the_media(&p, "film.mp4").unwrap().contains("web page"));
        assert_eq!(not_the_media(&p, "notes.html"), None);
        std::fs::write(&p, b"#EXTM3U\n#EXT-X-VERSION:3\n").unwrap();
        assert!(not_the_media(&p, "film.mp4").unwrap().contains("stream"));
        std::fs::write(&p, [0u8, 0, 0, 0x20, b'f', b't', b'y', b'p']).unwrap();
        assert_eq!(not_the_media(&p, "film.mp4"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod use_tests {
    use super::super::*;

    #[test]
    fn the_limit_only_while_the_computer_is_used() {
        let mut s = Settings { speed_limit_on: true, speed_limit_kb: 100, ..Default::default() };
        assert_eq!(limit_for(&s, Some(500)), 100 * 1024, "always, unless asked");
        s.limit_only_active = true;
        assert_eq!(limit_for(&s, Some(5)), 100 * 1024, "someone is at the computer");
        assert_eq!(limit_for(&s, Some(IDLE_AFTER)), 0, "nobody: full speed");
        assert_eq!(limit_for(&s, None), 100 * 1024, "not known: the limit");
    }
}
