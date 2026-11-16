use super::NAME;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The helper as a file named by its contents; older copies go (those a
/// browser still keeps open stay until next time).
pub(super) fn put_helper(dir: &Path, bytes: &[u8]) -> Option<PathBuf> {
    use sha2::Digest;
    let sum = sha2::Sha256::digest(bytes);
    let tag: String = sum.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let name = if cfg!(windows) { format!("aknan-host-{tag}.exe") } else { format!("aknan-host-{tag}") };
    let path = dir.join(&name);
    if std::fs::read(&path).map(|have| have != bytes).unwrap_or(true) {
        let tmp = dir.join(format!("{name}.new"));
        if std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("aknan-host-") && n != name {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Some(path)
}

pub(super) fn write_if_changed(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).map(|have| have == data).unwrap_or(false) {
        return Ok(());
    }
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

pub(super) fn read_ids(dir: &Path) -> BTreeSet<String> {
    std::fs::read(dir.join("ids.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|i| is_chrome_id(i))
        .collect()
}

pub(super) fn is_chrome_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

pub(super) fn chromium_manifest(host: &Path, ids: &BTreeSet<String>) -> String {
    let origins: Vec<String> = ids.iter().map(|i| format!("chrome-extension://{i}/")).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": NAME,
        "description": "Aknan Download",
        "path": host.display().to_string(),
        "type": "stdio",
        "allowed_origins": origins,
    }))
    .unwrap_or_default()
}

pub(super) fn firefox_manifest(host: &Path) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": NAME,
        "description": "Aknan Download",
        "path": host.display().to_string(),
        "type": "stdio",
        "allowed_extensions": [crate::browser::firefox_addon_id()],
    }))
    .unwrap_or_default()
}

/// The id Chrome gives an add-in loaded from `folder` (its full path, the
/// drive letter in capitals, hashed as Windows keeps the path: UTF-16).
pub(crate) fn chrome_id_for_path(folder: &Path) -> String {
    let mut s = folder.display().to_string();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let up = s[..1].to_ascii_uppercase();
        s.replace_range(..1, &up);
    }
    let bytes: Vec<u8> = if cfg!(windows) { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() } else { s.into_bytes() };
    chrome_id(&bytes)
}

pub(super) fn chrome_id(input: &[u8]) -> String {
    use sha2::Digest;
    let sum = sha2::Sha256::digest(input);
    sum.iter().take(16).flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect()
}
