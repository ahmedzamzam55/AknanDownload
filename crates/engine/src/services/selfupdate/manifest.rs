use serde::Deserialize;

pub const REPO: &str = "ahmedzamzam55/AknanDownload";
pub const SETUP_NAME: &str = "AknanDownload-Setup.exe";

/// Optional website `version.json` URL (e.g. `https://example.com/version.json`).
/// When non-empty (or overridden at build time via `AKNAN_UPDATE_URL`),
/// checked first before falling back to GitHub Releases.
pub const WEBSITE_VERSION_URL: &str = "";

pub fn version_url() -> String {
    format!("https://github.com/{REPO}/releases/latest/download/version.json")
}

/// Candidate `version.json` URLs in check order.
pub fn candidate_urls() -> Vec<String> {
    let mut urls = Vec::new();
    for custom in [option_env!("AKNAN_UPDATE_URL"), (!WEBSITE_VERSION_URL.is_empty()).then_some(WEBSITE_VERSION_URL)]
        .into_iter()
        .flatten()
    {
        let u = custom.trim();
        if u.starts_with("https://") && !urls.iter().any(|x| x == u) {
            urls.push(u.to_string());
        }
    }
    let gh = version_url();
    if !urls.iter().any(|x| x == &gh) {
        urls.push(gh);
    }
    urls
}

/// The running version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Public half of the release signing key (Ed25519, 32 bytes, hex).
pub const UPDATE_PUBLIC_KEY: &str = "c232fecde3678ec13986e81021795f33be086d384d85a6ea331d3141655229f8";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub url: String,
    pub sha256: String,
    /// Ed25519 signature of `signed_message(version, sha256)`, hex
    #[serde(default)]
    pub sig: String,
}

/// What the release signature covers.
pub fn signed_message(version: &str, sha256: &str) -> String {
    format!("aknan-download-update\n{}\n{}\n", version.trim(), sha256.trim().to_ascii_lowercase())
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// The signature of `r` was made with the key whose public half is `public_hex`.
pub fn signature_ok(r: &Release, public_hex: &str) -> bool {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let (Some(key), Some(sig)) = (from_hex(public_hex), from_hex(&r.sig)) else {
        return false;
    };
    if key.len() != 32 || sig.len() != 64 {
        return false;
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(signed_message(&r.version, &r.sha256).as_bytes(), &sig)
        .is_ok()
}

impl Release {
    /// Where the installer is downloaded from. Accepts any `https://` URL
    /// (website host, CDN, or GitHub Release) because the binary's SHA-256
    /// is verified against the Ed25519-signed `version.json`; otherwise falls
    /// back to the official GitHub Release URL.
    pub fn setup_url(&self) -> String {
        if self.url.starts_with("https://") {
            self.url.clone()
        } else {
            format!("https://github.com/{REPO}/releases/download/v{}/{SETUP_NAME}", self.version)
        }
    }
}

pub(super) fn resolve_setup_url(mut r: Release, manifest_url: &str) -> Release {
    if !r.url.is_empty() && !r.url.starts_with("https://") && !r.url.starts_with("http://") {
        if let Ok(base) = url::Url::parse(manifest_url) {
            if let Ok(joined) = base.join(&r.url) {
                if joined.scheme() == "https" {
                    r.url = joined.to_string();
                }
            }
        }
    }
    r
}

fn numbers(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// `remote` is a higher version than `local` ("1.0.10" > "1.0.9").
pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut a, mut b) = (numbers(remote), numbers(local));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

pub fn parse(text: &str) -> anyhow::Result<Release> {
    let r: Release = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if r.sha256.len() != 64 || !r.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("version.json has no valid sha256");
    }
    if numbers(&r.version).iter().all(|n| *n == 0) {
        anyhow::bail!("version.json has no valid version");
    }
    Ok(r)
}

/// `parse`, and the release must be signed with our key.
pub fn parse_signed(text: &str) -> anyhow::Result<Release> {
    let r = parse(text)?;
    if !signature_ok(&r, UPDATE_PUBLIC_KEY) {
        anyhow::bail!("the update is not signed by Aknan Download, so it is not installed");
    }
    Ok(r)
}
