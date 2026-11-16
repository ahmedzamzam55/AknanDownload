use super::control::{Control, FtpReply, Reply};
use super::site::Site;
use anyhow::bail;

/// What the server says about a file.
#[derive(Debug, Clone)]
pub struct Look {
    pub size: Option<u64>,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// a transfer can start in the middle: parts, resume
    pub rest: bool,
    /// the path the server took (relative to where the login lands, or from the top)
    pub path: String,
}

/// The file's path as this server takes it: from where the login lands (as
/// the address means it), else from the top.
async fn find(c: &mut Control, site: &Site) -> anyhow::Result<(String, Option<u64>)> {
    let relative = site.path.trim_start_matches('/').to_string();
    let mut last = None;
    for p in [relative, site.path.clone()] {
        let r = c.cmd(&format!("SIZE {p}")).await?;
        if r.code == 213 {
            return Ok((p, r.text.trim().parse().ok()));
        }
        // 550: not there; 500-502: no SIZE (the file may still be there)
        if (500..=502).contains(&r.code) {
            return Ok((p, None));
        }
        last = Some(r);
    }
    let r = last.unwrap_or(Reply { code: 550, text: String::new() });
    if r.code == 550 {
        bail!("the file was not found on the FTP server (550)");
    }
    Err(FtpReply { code: r.code, text: r.text }.into())
}

pub async fn look(url: &str, auth: Option<&(String, String)>) -> anyhow::Result<Look> {
    let site = Site::new(url, auth)?;
    let mut c = Control::connect(&site).await?;
    let (path, size) = find(&mut c, &site).await?;
    let modified = c.cmd(&format!("MDTM {path}")).await.ok().filter(|r| r.code == 213).and_then(|r| {
        let t = r.text.trim();
        chrono::NaiveDateTime::parse_from_str(t.get(..14)?, "%Y%m%d%H%M%S").ok().map(|d| d.and_utc())
    });
    let rest = c.cmd("REST 0").await.map(|r| r.code == 350).unwrap_or(false);
    let _ = c.cmd("QUIT").await;
    Ok(Look { size, modified, rest, path })
}

/// The download's file name from its address.
pub fn file_name(url: &str) -> String {
    let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or("");
    let name = percent_encoding::percent_decode_str(last).decode_utf8_lossy();
    let name = crate::filename::sanitize(&name);
    if name.is_empty() {
        "download".into()
    } else {
        name
    }
}
