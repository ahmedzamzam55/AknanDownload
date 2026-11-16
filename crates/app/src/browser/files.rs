use std::path::{Path, PathBuf};

/// The extension as it came with this program: when a file of its folder is
/// missing or was changed (an antivirus, a half-finished copy...), the
/// browser says "files missing or unreadable" and won't load it, so the
/// folder is put back in order from this copy first.
const EXTENSION_FILES: &[(&str, &[u8])] = &[
    ("early.js", include_bytes!("../../../../extension/early.js")),
    ("yt-itags.js", include_bytes!("../../../../extension/yt-itags.js")),
    ("yt-core.js", include_bytes!("../../../../extension/yt-core.js")),
    ("i18n.js", include_bytes!("../../../../extension/i18n.js")),
    ("background.js", include_bytes!("../../../../extension/background.js")),
    ("content.js", include_bytes!("../../../../extension/content.js")),
    ("popup-controls.js", include_bytes!("../../../../extension/popup-controls.js")),
    ("popup.js", include_bytes!("../../../../extension/popup.js")),
    ("popup.html", include_bytes!("../../../../extension/popup.html")),
    ("options.js", include_bytes!("../../../../extension/options.js")),
    ("options.html", include_bytes!("../../../../extension/options.html")),
    ("i18n/ar.js", include_bytes!("../../../../extension/i18n/ar.js")),
    ("i18n/de.js", include_bytes!("../../../../extension/i18n/de.js")),
    ("i18n/es.js", include_bytes!("../../../../extension/i18n/es.js")),
    ("i18n/fr.js", include_bytes!("../../../../extension/i18n/fr.js")),
    ("i18n/pt.js", include_bytes!("../../../../extension/i18n/pt.js")),
    ("i18n/ru.js", include_bytes!("../../../../extension/i18n/ru.js")),
    ("i18n/tr.js", include_bytes!("../../../../extension/i18n/tr.js")),
    ("bg/auth.js", include_bytes!("../../../../extension/bg/auth.js")),
    ("bg/context.js", include_bytes!("../../../../extension/bg/context.js")),
    ("bg/downloads.js", include_bytes!("../../../../extension/bg/downloads.js")),
    ("bg/helper.js", include_bytes!("../../../../extension/bg/helper.js")),
    ("bg/hls-sniff.js", include_bytes!("../../../../extension/bg/hls-sniff.js")),
    ("bg/intercept.js", include_bytes!("../../../../extension/bg/intercept.js")),
    ("bg/menus.js", include_bytes!("../../../../extension/bg/menus.js")),
    ("bg/messages.js", include_bytes!("../../../../extension/bg/messages.js")),
    ("bg/requests.js", include_bytes!("../../../../extension/bg/requests.js")),
    ("bg/resolve.js", include_bytes!("../../../../extension/bg/resolve.js")),
    ("bg/rules.js", include_bytes!("../../../../extension/bg/rules.js")),
    ("bg/selfupdate.js", include_bytes!("../../../../extension/bg/selfupdate.js")),
    ("bg/sniffer.js", include_bytes!("../../../../extension/bg/sniffer.js")),
    ("bg/tab-media.js", include_bytes!("../../../../extension/bg/tab-media.js")),
    ("bg/takeover.js", include_bytes!("../../../../extension/bg/takeover.js")),
    ("content/bubble-actions.js", include_bytes!("../../../../extension/content/bubble-actions.js")),
    ("content/bubble-dom.js", include_bytes!("../../../../extension/content/bubble-dom.js")),
    ("content/bubble-render.js", include_bytes!("../../../../extension/content/bubble-render.js")),
    ("content/clip-tab.js", include_bytes!("../../../../extension/content/clip-tab.js")),
    ("content/links.js", include_bytes!("../../../../extension/content/links.js")),
    ("content/place.js", include_bytes!("../../../../extension/content/place.js")),
    ("content/state.js", include_bytes!("../../../../extension/content/state.js")),
    ("content/styles.js", include_bytes!("../../../../extension/content/styles.js")),
    ("content/subs-tab.js", include_bytes!("../../../../extension/content/subs-tab.js")),
    ("content/videos.js", include_bytes!("../../../../extension/content/videos.js")),
    ("icons/icon16.png", include_bytes!("../../../../extension/icons/icon16.png")),
    ("icons/icon32.png", include_bytes!("../../../../extension/icons/icon32.png")),
    ("icons/icon48.png", include_bytes!("../../../../extension/icons/icon48.png")),
    ("icons/icon128.png", include_bytes!("../../../../extension/icons/icon128.png")),
];
const MANIFEST: &[u8] = include_bytes!("../../../../extension/manifest.json");
const MANIFEST_FIREFOX: &[u8] = include_bytes!("../../../../extension/manifest.firefox.json");

/// Every file of the extension in `dir`, as it came with the program (only
/// what differs is written). false = it could not be written there.
pub(crate) fn put_extension(dir: &Path, firefox: bool) -> bool {
    let manifest = if firefox { MANIFEST_FIREFOX } else { MANIFEST };
    // the program's secret key: an add-in loaded from this folder reads it
    // and shows it with every request (other programs don't get in)
    let key = format!(
        "{{\"token\":\"{}\",\"port\":{}}}\n",
        crate::api::TOKEN.get().cloned().unwrap_or_default(),
        crate::api::MAIN_PORT.load(std::sync::atomic::Ordering::Relaxed)
    );
    let mut all: Vec<(&str, &[u8])> = EXTENSION_FILES.to_vec();
    all.push(("manifest.json", manifest));
    if crate::api::TOKEN.get().is_some() {
        all.push(("token.json", key.as_bytes()));
    }
    let mut ok = true;
    for (name, data) in all {
        let path = dir.join(name);
        if std::fs::read(&path).map(|have| have == data).unwrap_or(false) {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // through a new file, so the browser never reads a half one
        let tmp = path.with_extension("aknan-new");
        if std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            ok = false;
        }
    }
    ok
}

/// At start: the add-in folders next to the program are complete and carry
/// the key (an add-in loaded from them before gets the key at once).
pub(crate) fn prepare_extension_folders() {
    // right now (not later): an add-in that reloads after an update reads
    // the key at once
    for firefox in [false, true] {
        let _ = extension_dir(firefox);
    }
}

/// Remove stray files and directories inside `dir` that are not part of the
/// shipped extension (`i18n`, `bg`, `content`, `icons` are kept and pruned
/// recursively).
fn prune_unknown(root: &Path, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(rel) = p.strip_prefix(root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            let prefix = format!("{rel}/");
            if EXTENSION_FILES.iter().any(|(f, _)| f.starts_with(&prefix)) {
                prune_unknown(root, &p);
            } else {
                let _ = std::fs::remove_dir_all(&p);
            }
        } else {
            let known = rel == "manifest.json" || rel == "token.json" || EXTENSION_FILES.iter().any(|(f, _)| *f == rel);
            if !known {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// "Repair the browser add-in": every file of the add-in folders is written
/// again as it came with the program, and the add-in (it sees the new
/// number) forgets what it kept and starts again.
pub(crate) fn repair_extension() -> bool {
    let mut ok = true;
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for (name, firefox) in [("extension", false), ("extension-firefox", true)] {
            let d = dir.join(name);
            if d.is_dir() {
                prune_unknown(&d, &d);
                ok &= put_extension(&d, firefox);
            }
        }
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
    crate::api::REPAIR.store(now, std::sync::atomic::Ordering::Relaxed);
    ok
}

/// The extension's folder next to the program (Firefox has its own copy),
/// complete. Where the program's folder can't be written, a copy in the
/// program's data folder is used.
pub(super) fn extension_dir(firefox: bool) -> Option<PathBuf> {
    let name = if firefox { "extension-firefox" } else { "extension" };
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let next_to = dir.join(name);
    if put_extension(&next_to, firefox) {
        return Some(next_to);
    }
    let own = swiftget_engine::default_data_dir().join(name);
    put_extension(&own, firefox).then_some(own)
}

/// The Chromium add-in folders of this program (next to it, and the copy in
/// its data folder where it could not write there): an add-in loaded from
/// one of them gets its id from that folder.
pub(crate) fn extension_folders() -> Vec<PathBuf> {
    let mut all = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        all.push(dir.join("extension"));
    }
    all.push(swiftget_engine::default_data_dir().join("extension"));
    all.into_iter().filter(|p| p.is_dir()).collect()
}

/// The id of our Firefox add-on (from its manifest).
pub(crate) fn firefox_addon_id() -> String {
    serde_json::from_slice::<serde_json::Value>(MANIFEST_FIREFOX)
        .ok()
        .and_then(|v| v["browser_specific_settings"]["gecko"]["id"].as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_keeps_subdirs_and_prunes_unknown() {
        let dir = std::env::temp_dir().join(format!("aknan-ext-repair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(put_extension(&dir, false));
        std::fs::create_dir_all(dir.join("stray_dir")).unwrap();
        std::fs::write(dir.join("stray_top.js"), b"x").unwrap();
        std::fs::write(dir.join("bg/stray_bg.js"), b"x").unwrap();
        std::fs::write(dir.join("token.json"), b"{}").unwrap();
        prune_unknown(&dir, &dir);
        assert!(!dir.join("stray_dir").exists());
        assert!(!dir.join("stray_top.js").exists());
        assert!(!dir.join("bg/stray_bg.js").exists());
        assert!(dir.join("token.json").is_file());
        assert!(put_extension(&dir, false));
        for (rel, _) in EXTENSION_FILES {
            assert!(dir.join(rel).is_file(), "missing {rel}");
        }
        assert!(dir.join("manifest.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
