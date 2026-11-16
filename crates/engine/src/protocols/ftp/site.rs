use super::ANSWER;
use crate::http::NetDrop;
use anyhow::{anyhow, bail, Context};
use tokio::io::{AsyncRead, AsyncWrite};

/// A connection that is plain or over TLS.
pub(super) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Where and who: from the address (and the download's login, if any).
#[derive(Debug, Clone)]
pub struct Site {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) user: String,
    pub(super) password: String,
    /// TLS from the first byte (ftps:// on its own port)
    pub(super) implicit: bool,
    /// TLS asked for ("AUTH TLS"): ftps:// on port 21
    pub(super) explicit: bool,
    /// the file's path as the address gives it (decoded)
    pub(super) path: String,
}

impl Site {
    pub fn new(url: &str, auth: Option<&(String, String)>) -> anyhow::Result<Site> {
        let u = url::Url::parse(url.trim()).context("not a valid FTP address")?;
        let tls = match u.scheme() {
            "ftp" => false,
            "ftps" => true,
            _ => bail!("not an FTP address"),
        };
        let host = u.host_str().filter(|h| !h.is_empty()).ok_or_else(|| anyhow!("the FTP address names no server"))?.to_string();
        let port = u.port().unwrap_or(if tls { 990 } else { 21 });
        let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned();
        let (user, password) = match auth.filter(|(u, _)| !u.is_empty()) {
            Some((u, p)) => (u.clone(), p.clone()),
            None if !u.username().is_empty() => (decode(u.username()), decode(u.password().unwrap_or(""))),
            None => ("anonymous".to_string(), "anonymous@".to_string()),
        };
        let path = decode(u.path());
        if path.trim_matches('/').is_empty() {
            bail!("the FTP address names no file");
        }
        if path.contains(['\r', '\n']) || user.contains(['\r', '\n']) || password.contains(['\r', '\n']) {
            bail!("not a valid FTP address");
        }
        Ok(Site { host, port, user, password, implicit: tls && port != 21, explicit: tls && port == 21, path })
    }

    /// The address without its password (what is kept in the list).
    pub fn without_password(url: &str) -> String {
        match url::Url::parse(url) {
            Ok(mut u) => {
                let _ = u.set_password(None);
                let _ = u.set_username("");
                u.to_string()
            }
            Err(_) => url.to_string(),
        }
    }

    /// The login an address carries ("ftp://user:pass@host/..."), if any.
    pub fn login_in(url: &str) -> Option<(String, String)> {
        let u = url::Url::parse(url).ok()?;
        let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned();
        (!u.username().is_empty()).then(|| (decode(u.username()), decode(u.password().unwrap_or(""))))
    }
}


pub(super) async fn timed<T>(f: impl std::future::Future<Output = std::io::Result<T>>) -> anyhow::Result<T> {
    match tokio::time::timeout(ANSWER, f).await {
        Ok(r) => Ok(r?),
        Err(_) => Err(NetDrop("the FTP server did not answer (timed out)").into()),
    }
}

#[cfg(windows)]
pub(super) async fn tls(stream: Box<dyn Io>, host: &str) -> anyhow::Result<Box<dyn Io>> {
    let cx = native_tls::TlsConnector::builder().build().context("TLS could not be started")?;
    let cx = tokio_native_tls::TlsConnector::from(cx);
    let s = tokio::time::timeout(ANSWER, cx.connect(host, stream))
        .await
        .map_err(|_| NetDrop("the FTP server did not answer (timed out)"))?
        .context("the secure connection to the FTP server failed")?;
    Ok(Box::new(s))
}

#[cfg(not(windows))]
pub(super) async fn tls(_stream: Box<dyn Io>, _host: &str) -> anyhow::Result<Box<dyn Io>> {
    bail!("FTP over TLS is not available here")
}

