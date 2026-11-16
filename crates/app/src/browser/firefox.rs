use super::detect::{detect, Browser};
use super::{open, open_unpacked_extension};
use crate::i18n::t;
use crate::*;
use swiftget_engine::http::SendOrdered;
use swiftget_engine::selfupdate;

/// The Firefox add-on signed by Mozilla (built by the release): installed
/// from this file it stays in Firefox for good.
const FIREFOX_XPI: &str = "aknan-download-firefox.xpi";

/// The signed Firefox add-on next to the program, when the release has one.
fn firefox_xpi() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(dir.join(FIREFOX_XPI)).filter(|p| p.exists()).or_else(|| Some(fetched_xpi()).filter(|p| p.exists()))
}

/// The signed Firefox add-on of this very version, next to the program or in data dir.
fn fetched_xpi() -> PathBuf {
    swiftget_engine::default_data_dir().join(format!("aknan-download-firefox-{}.xpi", env!("CARGO_PKG_VERSION")))
}

/// "Update the Firefox add-on": Firefox opens the signed add-on if available,
/// otherwise shows the browsers window.
pub(crate) fn update_firefox_addon(ui: &Ui) {
    let Some(exe) = detect().into_iter().find(|b| b.firefox).and_then(|b| b.exe) else { return open(ui) };
    if let Some(xpi) = firefox_xpi() {
        let _ = std::process::Command::new(&exe).arg(&xpi).spawn();
    } else {
        open(ui);
    }
}

fn firefox_xpi_urls() -> Vec<String> {
    let mut urls = Vec::new();
    for u in selfupdate::candidate_urls() {
        let gh = selfupdate::version_url();
        if u != gh {
            if let Some((dir, _)) = u.rsplit_once('/') {
                urls.push(format!("{dir}/{FIREFOX_XPI}"));
            }
        }
    }
    urls.push(format!(
        "https://github.com/{}/releases/download/v{}/{FIREFOX_XPI}",
        selfupdate::REPO,
        env!("CARGO_PKG_VERSION")
    ));
    urls.push(format!(
        "https://github.com/{}/releases/latest/download/{FIREFOX_XPI}",
        selfupdate::REPO
    ));
    urls
}

fn open_signed_firefox_xpi(exe: &Path, xpi: &Path, d: &BrowserDialog) {
    let _ = std::process::Command::new(exe).arg(xpi).spawn();
    d.set_folder(xpi.display().to_string().into());
    d.set_steps(t("Firefox opened the Aknan Download add-on.\n1. Click \"Add\" in the message Firefox shows.\n2. Then click \"OK\".\n\nThe add-on is signed by Mozilla, so it stays in Firefox after you close it and updates by itself.").into());
}

pub(super) fn add_firefox(ui: &Ui, b: &Browser, exe: &Path, d: &BrowserDialog) {
    if let Some(xpi) = firefox_xpi() {
        open_signed_firefox_xpi(exe, &xpi, d);
        return;
    }
    d.set_steps(t("Getting the signed Firefox add-on from the official page…").into());
    let client = ui.ctx.mgr.client();
    let weak_d = d.as_weak();
    let b_clone = b.clone();
    let exe = exe.to_path_buf();
    ui.ctx.rt.spawn(async move {
        let mut fetched: Option<Vec<u8>> = None;
        let mut last_err: Option<String> = None;
        for url in firefox_xpi_urls() {
            match client
                .get(&url)
                .header("User-Agent", swiftget_engine::config::DEFAULT_USER_AGENT)
                .send_ordered()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes().await {
                        Ok(bytes) if bytes.starts_with(b"PK\x03\x04") => {
                            fetched = Some(bytes.to_vec());
                            break;
                        }
                        Ok(_) => last_err = Some("invalid xpi".into()),
                        Err(e) => last_err = Some(e.to_string()),
                    }
                }
                Ok(resp) => last_err = Some(format!("HTTP {}", resp.status().as_u16())),
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        if let Some(bytes) = fetched {
            let dest = fetched_xpi();
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let tmp = dest.with_extension("xpi.tmp");
            if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &dest).is_ok() {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(d) = weak_d.upgrade() {
                        open_signed_firefox_xpi(&exe, &dest, &d);
                    } else {
                        let _ = std::process::Command::new(&exe).arg(&dest).spawn();
                    }
                });
                return;
            }
        }
        let _ = last_err;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(d) = weak_d.upgrade() {
                open_unpacked_extension(&b_clone, &exe, &d);
            }
        });
    });
}
