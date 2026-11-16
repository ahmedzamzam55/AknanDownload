use super::{Manager};
use crate::sync::RwOk;
use crate::http::SendOrdered;
use crate::config::Settings;
use crate::filename;
use crate::http::{self};
use crate::model::{DownloadKind, ProbeResult, RequestHeaders};
use crate::torrent;
use std::path::PathBuf;

impl Manager {

    // ---------------------------------------------------------------- actions

    /// Look at a URL without downloading it (for the "add" dialog).
    pub async fn probe(&self, url: &str, headers: &RequestHeaders) -> anyhow::Result<ProbeResult> {
        let (ua, headers) = {
            let s = self.settings.read_ok();
            (s.user_agent.clone(), with_login(&s, url, headers))
        };
        if crate::ftp::is_ftp(url) {
            return ftp_probe(url, &headers).await;
        }
        http::probe(&self.shared.client(), url, &headers, &ua).await
    }

    /// The HTTP client (with the proxy settings), for the site grabber.
    /// A .torrent file from a web address (a link clicked in the browser):
    /// fetched at once and kept in our data folder, so the torrent window can
    /// show what it shares. Never more than a few MB, and only a real
    /// torrent file is kept.
    pub async fn fetch_torrent_file(&self, url: &str, headers: &RequestHeaders) -> anyhow::Result<PathBuf> {
        const MAX: usize = 16 << 20;
        let ua = self.settings().user_agent.clone();
        let mut resp = http::request(&self.client(), url, headers, &ua).send_ordered().await?.error_for_status()?;
        if resp.content_length().is_some_and(|n| n > MAX as u64) {
            anyhow::bail!("too big for a .torrent file");
        }
        let mut data = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            data.extend_from_slice(&chunk);
            if data.len() > MAX {
                anyhow::bail!("too big for a .torrent file");
            }
        }
        let info = torrent::parse_torrent(&data)?;
        let dir = self.data_dir.join("torrents").join("from-browser");
        std::fs::create_dir_all(&dir)?;
        // a name of our own: never a path from the server
        let stem: String = info.name.chars().filter(|c| c.is_alphanumeric() || " -_.()[]".contains(*c)).take(80).collect();
        let stem = if stem.trim().is_empty() { "download".to_string() } else { stem.trim().to_string() };
        let path = dir.join(filename::unique_name(&dir, &format!("{stem}.torrent")));
        std::fs::write(&path, &data)?;
        Ok(path)
    }

    pub fn client(&self) -> reqwest::Client {
        self.shared.client()
    }
}

/// The download's headers plus a saved site login, if it has none of its own.
pub(super) fn with_login(s: &Settings, url: &str, h: &RequestHeaders) -> RequestHeaders {
    let mut h = h.clone();
    if h.auth.is_none() {
        h.auth = s.login_for(url);
    }
    h
}

/// What an FTP server says about the file, as a look at an http address
/// says it.
pub(super) async fn ftp_probe(url: &str, headers: &RequestHeaders) -> anyhow::Result<ProbeResult> {
    let auth = headers.auth.clone().or_else(|| crate::ftp::Site::login_in(url));
    let look = crate::ftp::look(url, auth.as_ref()).await?;
    Ok(ProbeResult {
        final_url: crate::ftp::Site::without_password(url),
        file_name: crate::ftp::file_name(url),
        total_size: look.size,
        resumable: look.rest && look.size.is_some(),
        content_type: None,
        kind: DownloadKind::Http,
        last_modified: look.modified,
        etag: None,
        headers_used: None,
        digest: None,
    })
}
