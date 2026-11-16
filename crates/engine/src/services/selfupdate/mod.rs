//! New versions of Aknan Download itself.
//!
//! Every release on GitHub carries two files:
//!
//! * `AknanDownload-Setup.exe` – the installer
//! * `version.json`           – `{"version":"1.0.1","url":"…/AknanDownload-Setup.exe","sha256":"…"}`
//!
//! The app reads `version.json` of the latest release, and when the version
//! there is higher than its own it downloads the installer, checks that its
//! SHA-256 matches, and hands it back to the app to run.
//!
//! `version.json` also carries `sig`: an Ed25519 signature of the version and
//! the SHA-256, made by the release build with a private key that only lives
//! in the repository's secrets. The public key is built into the program, so
//! a release (or a file) that was not made with that key is never installed,
//! even if someone could publish files on the repository.

use crate::http::SendOrdered;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

mod manifest;
#[cfg(test)]
mod tests;

pub use manifest::{
    candidate_urls, current, from_hex, is_newer, parse, parse_signed, signature_ok, signed_message,
    version_url, Release, REPO, SETUP_NAME, UPDATE_PUBLIC_KEY, WEBSITE_VERSION_URL,
};
use manifest::resolve_setup_url;

/// The latest release, if it is newer than this program.
pub async fn check(client: &reqwest::Client) -> anyhow::Result<Option<Release>> {
    let mut last_err: Option<anyhow::Error> = None;
    let mut only_404 = true;
    for manifest_url in candidate_urls() {
        let resp = match client
            .get(&manifest_url)
            .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send_ordered()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                only_404 = false;
                last_err = Some(e.into());
                continue;
            }
        };
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            last_err = Some(anyhow::anyhow!("HTTP 404 at {manifest_url}"));
            continue;
        }
        let resp = match resp.error_for_status() {
            Ok(r) => r,
            Err(e) => {
                only_404 = false;
                last_err = Some(e.into());
                continue;
            }
        };
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                only_404 = false;
                last_err = Some(e.into());
                continue;
            }
        };
        let r: Release = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
        if !is_newer(&r.version, current()) {
            return Ok(None); // nothing newer: older unsigned files don't matter
        }
        let r = parse_signed(&text)?;
        return Ok(Some(resolve_setup_url(r, &manifest_url)));
    }
    if only_404 {
        return Ok(None);
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no update manifest URLs configured")))
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// No installer of ours is anywhere near this big.
const MAX_SETUP_BYTES: u64 = 500 << 20;

/// Download the installer of `r` into `dir` and check its SHA-256.
/// A copy that was already downloaded and checked is reused.
pub async fn download(client: &reqwest::Client, r: &Release, dir: &Path) -> anyhow::Result<PathBuf> {
    download_with_progress(client, r, dir, &|_, _| {}).await
}

/// `download`, telling `progress(bytes so far, total)` while it goes.
pub async fn download_with_progress(
    client: &reqwest::Client,
    r: &Release,
    dir: &Path,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(format!("AknanDownload-Setup-{}.exe", r.version.trim_start_matches('v')));
    if dest.exists() {
        if file_sha256(&dest)?.eq_ignore_ascii_case(&r.sha256) {
            return Ok(dest);
        }
        let _ = std::fs::remove_file(&dest);
    }
    // older installers are not needed any more
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let mut resp = client
        .get(r.setup_url())
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .send_ordered()
        .await?
        .error_for_status()?;
    use std::io::Write;
    let tmp = dest.with_extension("download");
    let mut file = std::fs::File::create(&tmp)?;
    let mut h = Sha256::new();
    let mut total: u64 = 0;
    let size = resp.content_length();
    progress(0, size);
    while let Some(chunk) = resp.chunk().await? {
        total += chunk.len() as u64;
        progress(total, size);
        if total > MAX_SETUP_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("the update is far bigger than expected");
        }
        h.update(&chunk);
        file.write_all(&chunk)?;
    }
    file.flush()?;
    drop(file);
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(&r.sha256) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("the downloaded installer does not match version.json (sha256 {got})");
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Run the installer without any window. It closes this program, installs
/// the new version over it and starts it again.
/// `visible`: the setup shows its progress (Help > Check for updates);
/// otherwise nothing is shown (the automatic update).
pub fn run_installer(setup: &Path, sha256: &str, visible: bool) -> anyhow::Result<()> {
    // checked again right before it runs: the file sat on the disk meanwhile
    if !file_sha256(setup)?.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(setup);
        anyhow::bail!("the downloaded update was changed on the disk, so it is not installed");
    }
    let mut cmd = std::process::Command::new(setup);
    cmd.arg(if visible { "/SILENT" } else { "/VERYSILENT" });
    cmd.args(["/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/AUTOUPDATE=1"]);
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        cmd.arg(format!("/DIR={}", dir.display()));
    }
    cmd.spawn()?;
    Ok(())
}

/// Installed with the installer (not a portable copy): only then the
/// installer may replace the program.
pub fn installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("unins000.exe").exists()))
        .unwrap_or(false)
}
