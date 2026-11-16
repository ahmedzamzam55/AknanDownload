use super::files::{chrome_id, chromium_manifest, is_chrome_id, put_helper};
use super::{chrome_id_for_path, NAME};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn ids_as_chrome_makes_them() {
    // Chromium's own test value (crx_file id_util)
    assert_eq!(chrome_id(b"test"), "jpignaibiiemhngfjkcpokkamffknabf");
    assert!(is_chrome_id("jpignaibiiemhngfjkcpokkamffknabf"));
    assert!(!is_chrome_id("jpignaibiiemhngfjkcpokkamffknabz"));
    // the drive letter is the same either way
    assert_eq!(chrome_id_for_path(Path::new(r"c:\x\extension")), chrome_id_for_path(Path::new(r"C:\x\extension")));
}

#[test]
fn the_helper_is_put_in_place() {
    let dir = std::env::temp_dir().join(format!("aknan-host-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("aknan-host-old"), b"old").unwrap();
    let a = put_helper(&dir, b"one").unwrap();
    assert!(a.is_file());
    assert!(!dir.join("aknan-host-old").exists(), "an older copy goes");
    let b = put_helper(&dir, b"two").unwrap();
    assert_ne!(a, b, "new contents, a new name (the old one may be open)");
    assert_eq!(put_helper(&dir, b"two").unwrap(), b);
    let ids: BTreeSet<String> = ["jpignaibiiemhngfjkcpokkamffknabf".to_string()].into();
    let m: serde_json::Value = serde_json::from_str(&chromium_manifest(&b, &ids)).unwrap();
    assert_eq!(m["allowed_origins"][0], "chrome-extension://jpignaibiiemhngfjkcpokkamffknabf/");
    assert_eq!(m["name"], NAME);
    let _ = std::fs::remove_dir_all(&dir);
}
