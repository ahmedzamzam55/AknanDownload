use super::GrabOptions;
use crate::filename;
use crate::http::{request, SendOrdered};
use crate::model::RequestHeaders;
use reqwest::Client;

/// What an address gave: a page to read, or a file (its name from the server).
pub(super) enum Fetched {
    Page(String, String),
    File { url: String, name: String },
}

/// The name a file answer gives itself (the server's, else its address's).
fn answer_name(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename::from_content_disposition)
        .or_else(|| filename::from_url(resp.url().as_str()))
        .map(|n| filename::sanitize(&n))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "download".into())
}

fn is_page_type(ct: &str) -> bool {
    ct.is_empty() || ct.contains("html") || ct.contains("xml")
}

pub(super) async fn fetch_page(client: &Client, url: &str, opts: &GrabOptions) -> Option<Fetched> {
    let resp = request(client, url, &opts.headers, &opts.user_agent).send_ordered().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let attached = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("attachment"));
    // an address with no file name of its own ("get.php?id=5", "/file/abc")
    // that answers with a file: a file (only its first bytes were read)
    if attached || !is_page_type(&ct) {
        // no extension in the name: the one of its type ("77" + application/pdf)
        let name = filename::ensure_extension(answer_name(&resp), Some(&ct));
        return Some(Fetched::File { url: resp.url().to_string(), name });
    }
    let final_url = resp.url().to_string();
    // read at most 4 MB of a page
    let mut resp = resp;
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > 4 * 1024 * 1024 {
            break;
        }
    }
    Some(Fetched::Page(final_url, String::from_utf8_lossy(&body).into_owned()))
}

/// What a link turned out to be when the server was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Checked {
    /// a file: the server's name for it and its size
    File { name: String, size: Option<u64> },
    /// a web page
    Page,
    /// no good (not found, refused...): why, in short
    Gone(String),
}

/// Ask the server what a link is, without downloading it.
pub async fn check_link(client: &Client, url: &str, headers: &RequestHeaders, ua: &str) -> Checked {
    let asked = tokio::time::timeout(std::time::Duration::from_secs(20), crate::http::probe_quick(client, url, headers, ua)).await;
    crate::http::drop_kept(url);
    match asked {
        Ok(Ok(p)) => {
            let ct = p.content_type.clone().unwrap_or_default().to_ascii_lowercase();
            if (ct.contains("html") || ct.contains("xhtml")) && p.kind == crate::model::DownloadKind::Http {
                Checked::Page
            } else {
                Checked::File { name: p.file_name.clone(), size: p.total_size }
            }
        }
        Ok(Err(e)) => Checked::Gone(format!("{e:#}").chars().take(120).collect()),
        Err(_) => Checked::Gone("no answer".into()),
    }
}
