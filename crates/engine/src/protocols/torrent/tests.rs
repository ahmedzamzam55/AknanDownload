use super::*;
use std::path::Path;

fn ben_str(s: &str) -> String {
    format!("{}:{s}", s.len())
}

#[test]
fn reads_a_multi_file_torrent() {
    let t = format!(
        "d8:announce{}4:infod5:filesld6:lengthi100e4:pathl{}{}eed6:lengthi5e4:pathl{}eed4:attr1:p6:lengthi7e4:pathl{}eee4:name{}12:piece lengthi16384e6:pieces0:ee",
        ben_str("http://t/a"),
        ben_str("Season 1"),
        ben_str("ep1.mkv"),
        ben_str("readme.txt"),
        ben_str(".pad"),
        ben_str("My Show")
    );
    let i = parse_torrent(t.as_bytes()).unwrap();
    assert_eq!(i.name, "My Show");
    assert_eq!(i.files, vec![
        TorrentFile { path: "Season 1/ep1.mkv".into(), size: 100, index: 1 },
        TorrentFile { path: "readme.txt".into(), size: 5, index: 2 },
    ]);
    assert_eq!(i.total, 105);
    assert!(i.multi);
    assert_eq!(i.trackers, vec!["http://t/a".to_string()]);
    assert_eq!(i.info_hash.as_deref().map(str::len), Some(40));
    assert!(safe_paths(&i).is_empty(), "nothing to change");
}

#[test]
fn unsafe_names_are_written_safely() {
    let t = format!(
        "d4:infod5:filesld6:lengthi1e4:pathl{}{}eed6:lengthi1e4:pathl{}eed6:lengthi1e4:pathl{}eee4:name{}ee",
        ben_str(".."),
        ben_str("evil.exe"),
        ben_str("CON"),
        ben_str("ok.txt"),
        ben_str("Show")
    );
    let i = parse_torrent(t.as_bytes()).unwrap();
    let out = safe_paths(&i);
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0], (1, "Show/_/evil.exe".to_string()));
    assert!(!out[1].1.ends_with("/CON"), "{out:?}");
    assert!(out.iter().all(|(_, p)| !p.split('/').any(|x| x == "..")));
    // a torrent whose own name leads out
    let t = format!("d4:infod6:lengthi3e4:name{}ee", ben_str("../../x.exe"));
    let i = parse_torrent(t.as_bytes()).unwrap();
    let out = safe_paths(&i);
    assert_eq!(out.len(), 1);
    assert!(!out[0].1.contains(['/', '\\']) && out[0].1 != "..", "a name, never a way out: {out:?}");
    assert!(stays_inside(Path::new("/d/a/b.txt"), Path::new("/d")));
    assert!(!stays_inside(Path::new("/d/../e.txt"), Path::new("/d")));
    assert!(!stays_inside(Path::new("/e/x"), Path::new("/d")));
    assert!(stays_inside(Path::new("/d/./a/b"), Path::new("/d/")));
}

#[test]
fn magnets_trackers_and_pieces() {
    let t = format!("d8:announce{}4:infod6:lengthi3e4:name{}ee", ben_str("udp://t.test:1/a"), ben_str("a b"));
    let m = magnet_of(t.as_bytes()).unwrap();
    assert!(m.starts_with("magnet:?xt=urn:btih:") && m.contains("&dn=a%20b") && m.contains("&tr=udp%3A%2F%2Ft%2Etest%3A1%2Fa"), "{m}");
    assert!(is_magnet(&m));
    assert!(is_tracker("udp://tracker.test:80/announce"));
    assert!(!is_tracker("udp://a,b"));
    assert!(!is_tracker("file:///etc/passwd"));
    assert!((share_of("f0") - 0.5).abs() < 1e-6);
    // 8 pieces, the first 4 here
    let runs = piece_runs("f0", 8, 100);
    assert_eq!(runs, vec![(0.0, 0.5, true), (0.5, 1.0, false)]);
}

#[test]
fn reads_a_single_file_torrent_and_refuses_junk() {
    let t = format!("d4:infod6:lengthi42e4:name{}ee", ben_str("film.mp4"));
    let i = parse_torrent(t.as_bytes()).unwrap();
    assert_eq!(i.files, vec![TorrentFile { path: "film.mp4".into(), size: 42, index: 1 }]);
    assert!(parse_torrent(b"<html>").is_err());
    assert!(parse_torrent(b"d4:infod6:lengthi42e4:name99:x").is_err());
    assert!(parse_torrent(&b"l".repeat(200)).is_err());
}

#[test]
fn magnet_links() {
    let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Film%20(2024)&tr=udp%3A%2F%2Ft";
    assert!(is_magnet(m));
    assert!(is_torrent_source(m));
    assert!(is_torrent_source(r"C:\Users\a\Downloads\x.torrent"));
    assert!(!is_torrent_source("https://site/x.torrent"));
    assert!(!is_magnet("magnet:?xt=urn:sha1:abc"));
    assert_eq!(magnet_name(m).as_deref(), Some("Big Film (2024)"));
    assert_eq!(magnet_name("magnet:?xt=urn:btih:00"), None);
}

