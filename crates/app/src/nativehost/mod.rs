//! The browser talks to the program through a small helper of ours that the
//! browser starts itself ("native messaging", aknan-host.exe), not over the
//! network: nothing the browser does about web pages reaching this computer,
//! and no port another program (or Windows) keeps, can stop it.
//!
//! At start the program puts the helper in its data folder (a copy named by
//! its contents: the one a browser keeps open is never written over, so an
//! update never waits for the browser), says where the program is, and tells
//! the browsers about it (in the user's own part of the registry). The port
//! stays as it was, for the add-ins that don't use the helper (yet).

mod files;
#[cfg(test)]
mod tests;

pub(crate) use files::chrome_id_for_path;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use swiftget_engine::sync::LockOk;

/// The helper's name for the browsers.
pub(crate) const NAME: &str = "com.aknan.download";

/// Where each browser looks for helpers (the key's default value is the
/// helper's description file). Opera and Vivaldi read Chrome's.
#[cfg(windows)]
const CHROMIUM_KEYS: &[&str] = &[
    r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.aknan.download",
    r"HKCU\Software\Chromium\NativeMessagingHosts\com.aknan.download",
    r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.aknan.download",
    r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.aknan.download",
];
#[cfg(windows)]
const FIREFOX_KEY: &str = r"HKCU\Software\Mozilla\NativeMessagingHosts\com.aknan.download";

struct State {
    dir: PathBuf,
    /// the add-ins of Chromium browsers that may start the helper
    ids: BTreeSet<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn host_file_name() -> &'static str {
    if cfg!(windows) {
        "aknan-host.exe"
    } else {
        "aknan-host"
    }
}

/// At start (on its own thread): the helper in place and the browsers told.
pub(crate) fn install(data_dir: &Path) {
    let Some(exe) = std::env::current_exe().ok() else { return };
    let Some(src) = exe.parent().map(|d| d.join(host_file_name())).filter(|p| p.is_file()) else {
        return; // a build without the helper: the add-in keeps the port
    };
    let Ok(bytes) = std::fs::read(&src) else { return };
    let dir = data_dir.join("host");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Some(host) = files::put_helper(&dir, &bytes) else { return };
    let app = serde_json::json!({
        "app": exe.display().to_string(),
        "settings": data_dir.join("settings.json").display().to_string(),
    });
    let _ = files::write_if_changed(&dir.join("app.json"), app.to_string().as_bytes());

    // the add-in folders of this program: their ids are known before the
    // add-in ever says it (Chrome names an add-in loaded from a folder after
    // the folder); ids the add-ins said themselves are kept too
    let mut ids: BTreeSet<String> = files::read_ids(&dir);
    for folder in crate::browser::extension_folders() {
        ids.insert(chrome_id_for_path(&folder));
    }
    let _ = files::write_if_changed(&dir.join("chromium.json"), files::chromium_manifest(&host, &ids).as_bytes());
    let _ = files::write_if_changed(&dir.join("firefox.json"), files::firefox_manifest(&host).as_bytes());
    *STATE.lock_ok() = Some(State { dir: dir.clone(), ids });

    #[cfg(windows)]
    {
        let chromium = dir.join("chromium.json").display().to_string();
        let firefox = dir.join("firefox.json").display().to_string();
        for key in CHROMIUM_KEYS {
            point(key, &chromium);
        }
        point(FIREFOX_KEY, &firefox);
    }
}

/// What the link test says about the helper: is it in place, and which
/// browsers know it. (browser, it knows the helper)
pub(crate) fn report() -> (bool, Vec<(&'static str, bool)>) {
    let st = STATE.lock_ok();
    let Some(st) = st.as_ref() else { return (false, Vec::new()) };
    let helper = std::fs::read(st.dir.join("chromium.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from))
        .is_some_and(|p| p.is_file());
    #[cfg(windows)]
    let browsers = {
        let chromium = st.dir.join("chromium.json").display().to_string();
        let firefox = st.dir.join("firefox.json").display().to_string();
        let names = ["Chrome", "Chromium", "Edge", "Brave"];
        let mut out: Vec<(&'static str, bool)> =
            CHROMIUM_KEYS.iter().zip(names).map(|(k, n)| (n, crate::assoc::query(k, None).as_deref() == Some(chromium.as_str()))).collect();
        out.push(("Firefox", crate::assoc::query(FIREFOX_KEY, None).as_deref() == Some(firefox.as_str())));
        out
    };
    #[cfg(not(windows))]
    let browsers = Vec::new();
    (helper, browsers)
}

/// The key of a browser points at `file` (written only when it does not).
#[cfg(windows)]
fn point(key: &str, file: &str) {
    if crate::assoc::query(key, None).as_deref() != Some(file) {
        crate::assoc::reg(&["add", key, "/ve", "/d", file, "/f"]);
    }
}

/// A Chromium add-in said who it is (each request): a new one may start the
/// helper from now on.
pub(crate) fn saw_addin(id: &str) {
    if !files::is_chrome_id(id) {
        return;
    }
    let mut st = STATE.lock_ok();
    let Some(st) = st.as_mut() else { return };
    if !st.ids.insert(id.to_string()) {
        return;
    }
    let ids: Vec<&String> = st.ids.iter().collect();
    let _ = files::write_if_changed(&st.dir.join("ids.json"), serde_json::to_string(&ids).unwrap_or_default().as_bytes());
    let host = std::fs::read(st.dir.join("chromium.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from));
    if let Some(host) = host {
        let _ = files::write_if_changed(&st.dir.join("chromium.json"), files::chromium_manifest(&host, &st.ids).as_bytes());
    }
}
