use super::*;

fn md5_hex(data: &[u8]) -> String {
    let mut m = Md5::new();
    m.update(data);
    to_hex(&m.finish())
}

#[test]
fn md5_is_right() {
    assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(md5_hex(b"The quick brown fox jumps over the lazy dog"), "9e107d9d372bb6826bd81d3542a419d6");
    // in pieces of every size: the same
    let long: Vec<u8> = (0..10_000u32).map(|i| (i % 253) as u8).collect();
    let whole = md5_hex(&long);
    for step in [1, 7, 63, 64, 65, 1000] {
        let mut m = Md5::new();
        for c in long.chunks(step) {
            m.update(c);
        }
        assert_eq!(to_hex(&m.finish()), whole);
    }
}

#[test]
fn what_users_paste() {
    let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(Expected::parse(sha).unwrap().algo, Algo::Sha256);
    assert_eq!(Expected::parse(&format!("SHA-256: {}", sha.to_uppercase())).unwrap().hex, sha);
    assert_eq!(Expected::parse(&format!("{sha} *ubuntu.iso")).unwrap().algo, Algo::Sha256);
    assert_eq!(Expected::parse("md5=d41d8cd98f00b204e9800998ecf8427e").unwrap().algo, Algo::Md5);
    assert!(Expected::parse("sha1:d41d8cd98f00b204e9800998ecf8427e").is_none(), "an MD5 called SHA-1");
    assert!(Expected::parse("hello").is_none());
    assert!(Expected::parse("").is_none());
}

#[test]
fn what_servers_send() {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("repr-digest", "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:".parse().unwrap());
    let e = Expected::from_headers(&h, false).unwrap();
    assert_eq!(e.text(), "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("content-md5", "1B2M2Y8AsgTpgAmY7PhCfg==".parse().unwrap());
    assert!(Expected::from_headers(&h, false).is_none(), "a part's own");
    assert_eq!(Expected::from_headers(&h, true).unwrap().hex, "d41d8cd98f00b204e9800998ecf8427e");
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-goog-hash", "crc32c=AAAAAA==, md5=1B2M2Y8AsgTpgAmY7PhCfg==".parse().unwrap());
    assert_eq!(Expected::from_headers(&h, false).unwrap().algo, Algo::Md5);
}

#[test]
fn a_file_is_checked() {
    let p = std::env::temp_dir().join(format!("aknan-sum-{}", std::process::id()));
    std::fs::write(&p, b"abc").unwrap();
    let right = Expected::parse("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap();
    assert!(matches(&p, &right, &|_| {}, &|| false).unwrap());
    let md5 = Expected::parse("900150983cd24fb0d6963f7d28e17f72").unwrap();
    assert!(matches(&p, &md5, &|_| {}, &|| false).unwrap());
    let wrong = Expected::parse("0000000000000000000000000000000000000000").unwrap();
    assert!(!matches(&p, &wrong, &|_| {}, &|| false).unwrap());
    let _ = std::fs::remove_file(&p);
}
