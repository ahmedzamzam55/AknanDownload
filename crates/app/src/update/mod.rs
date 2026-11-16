//! New versions of Aknan Download from GitHub or the official website, always on:
//! right after the program opens it looks for a newer release; when there is one it is
//! downloaded, checked (signature of the release + SHA-256) and installed silently, and the program
//! starts again by itself. While the program stays open it looks again every
//! few hours (then it waits until no download is running).
//! Help > Check for updates does the same at once in a window of its own
//! "checking…", "this is the newest version", or the download of
//! the new one with its progress, then its setup runs in front of the user.

mod dialog;

pub use dialog::check_now;

use crate::i18n::tf;
use crate::message;
use crate::Ctx;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{selfupdate, Manager, Status};

/// one check / download at a time (automatic or from the menu)
pub(super) static BUSY: AtomicBool = AtomicBool::new(false);

pub(super) fn update_dir() -> PathBuf {
    swiftget_engine::default_data_dir().join("update")
}

fn downloading(mgr: &Manager) -> bool {
    mgr.snapshot()
        .iter()
        .any(|v| matches!(v.status, Status::Connecting | Status::Downloading | Status::Assembling))
}

pub(super) fn apply_as(setup: PathBuf, sha256: String, visible: bool) {
    match selfupdate::run_installer(&setup, &sha256, visible) {
        Ok(()) => {
            let _ = slint::quit_event_loop();
        }
        Err(e) => message(&tf("Could not start the update:\n\n{}", &[&format!("{e:#}")])),
    }
}

pub fn spawn(ctx: &Ctx) {
    // leftovers of an update that is installed now
    if let Ok(rd) = std::fs::read_dir(update_dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.contains(&format!("-{}.", selfupdate::current())) || name.ends_with(".download") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if let Ok(t) = std::fs::read_to_string(update_dir().join("tries.txt")) {
        let v = t.split(' ').next().unwrap_or_default().to_string();
        if !selfupdate::is_newer(&v, selfupdate::current()) {
            // that version (or a newer one) is what runs now
            let _ = std::fs::remove_file(update_dir().join("tries.txt"));
        }
    }
    if !selfupdate::installed() {
        return; // a portable copy: there is no installation to update
    }
    let mgr = ctx.mgr.clone();
    ctx.rt.spawn(async move {
        // first check as soon as the program is open
        tokio::time::sleep(Duration::from_secs(3)).await;
        let mut first = true;
        loop {
            if !BUSY.swap(true, Ordering::SeqCst) {
                let started = round(&mgr, first).await;
                BUSY.store(false, Ordering::SeqCst);
                if started {
                    return;
                }
            }
            first = false;
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// How many times the setup of `version` was started already (it closes
/// the program and should open the new version again; when the program
/// opens as the old version again, that setup did not get through).
fn tries_of(version: &str) -> u32 {
    std::fs::read_to_string(update_dir().join("tries.txt"))
        .ok()
        .and_then(|t| {
            let (v, n) = t.trim().split_once(' ')?;
            (v == version).then(|| n.parse().ok()).flatten()
        })
        .unwrap_or(0)
}

fn count_try(version: &str) {
    let n = tries_of(version) + 1;
    let _ = std::fs::create_dir_all(update_dir());
    let _ = std::fs::write(update_dir().join("tries.txt"), format!("{version} {n}"));
}

/// One check. true = the installer was started.
async fn round(mgr: &Arc<Manager>, at_start: bool) -> bool {
    let client = mgr.client();
    let Ok(Some(rel)) = selfupdate::check(&client).await else {
        return false;
    };
    let Ok(setup) = selfupdate::download(&client, &rel, &update_dir()).await else {
        return false;
    };
    // later checks never cut a running download short; at start the
    // downloads simply go on after the update
    if !at_start {
        while downloading(mgr) {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
    let sha = rel.sha256.clone();
    let tries = tries_of(&rel.version);
    if tries >= 2 {
        // twice started and still the old version here: no more closing the
        // program by itself; Help > Check for updates installs it in front of
        // the user (and shows why it can't)
        crate::note(&format!("update {} did not install after {tries} tries; waiting for the user", rel.version));
        return false;
    }
    count_try(&rel.version);
    let version = rel.version.clone();
    let _ = slint::invoke_from_event_loop(move || {
        crate::note(&format!("installing update {version} (try {})", tries + 1));
        // its small progress window shows: the program closing by itself
        // without it looked like a crash
        apply_as(setup, sha, true)
    });
    true
}
