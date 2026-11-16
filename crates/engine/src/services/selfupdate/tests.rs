use super::manifest::resolve_setup_url;
use super::*;


#[test]
fn versions() {
    assert!(is_newer("1.0.1", "1.0.0"));
    assert!(is_newer("v1.1", "1.0.9"));
    assert!(is_newer("1.0.10", "1.0.9"));
    assert!(is_newer("2.0.0", "1.99.99"));
    assert!(!is_newer("1.0.0", "1.0.0"));
    assert!(!is_newer("1.0", "1.0.0"));
    assert!(!is_newer("0.9.9", "1.0.0"));
    assert!(!is_newer("garbage", "1.0.0"));
}

#[test]
fn parse_and_url() {
    let sha = sha256_hex(b"abc");
    assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let r = parse(&format!("\u{feff}{{\"version\":\"1.0.1\",\"sha256\":\"{sha}\"}}")).unwrap();
    assert_eq!(r.setup_url(), format!("https://github.com/{REPO}/releases/download/v1.0.1/{SETUP_NAME}"));
    let r2 = parse(&format!("{{\"version\":\"1.0.1\",\"url\":\"https://aknan.example/setup.exe\",\"sha256\":\"{sha}\"}}")).unwrap();
    assert_eq!(r2.setup_url(), "https://aknan.example/setup.exe");
    let r_http = parse(&format!("{{\"version\":\"1.0.1\",\"url\":\"http://insecure.example/setup.exe\",\"sha256\":\"{sha}\"}}")).unwrap();
    assert_eq!(r_http.setup_url(), format!("https://github.com/{REPO}/releases/download/v1.0.1/{SETUP_NAME}"));
    let r_rel = resolve_setup_url(
        parse(&format!("{{\"version\":\"1.0.1\",\"url\":\"AknanDownload-Setup.exe\",\"sha256\":\"{sha}\"}}")).unwrap(),
        "https://aknan.example/downloads/version.json",
    );
    assert_eq!(r_rel.setup_url(), "https://aknan.example/downloads/AknanDownload-Setup.exe");
    assert!(parse("{\"version\":\"1.0.1\",\"sha256\":\"zz\"}").is_err());
}

fn signed(version: &str, sha: &str) -> (String, String) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
    let public: String = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let sig: String = kp.sign(signed_message(version, sha).as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    (public, sig)
}

#[test]
fn signatures() {
    let sha = sha256_hex(b"setup");
    let (public, sig) = signed("1.0.5", &sha);
    let good = Release { version: "1.0.5".into(), url: String::new(), sha256: sha.clone(), sig: sig.clone() };
    assert!(signature_ok(&good, &public));
    // another version, another file, no signature, a broken one: refused
    assert!(!signature_ok(&Release { version: "1.0.6".into(), ..good.clone() }, &public));
    assert!(!signature_ok(&Release { sha256: sha256_hex(b"evil"), ..good.clone() }, &public));
    assert!(!signature_ok(&Release { sig: String::new(), ..good.clone() }, &public));
    assert!(!signature_ok(&Release { sig: "zz".repeat(64), ..good.clone() }, &public));
    // signed with another key: refused by the key in the program
    assert!(!signature_ok(&good, UPDATE_PUBLIC_KEY));
    let text = format!("{{\"version\":\"1.0.5\",\"sha256\":\"{sha}\",\"sig\":\"{sig}\"}}");
    assert!(parse_signed(&text).is_err());
    assert_eq!(UPDATE_PUBLIC_KEY.len(), 64);
}

#[test]
fn installer_checked_again_before_it_runs() {
    let dir = std::env::temp_dir().join(format!("aknan-su-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("setup.exe");
    std::fs::write(&f, b"changed").unwrap();
    let err = run_installer(&f, &sha256_hex(b"original"), false).unwrap_err();
    assert!(format!("{err}").contains("changed"));
    assert!(!f.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
