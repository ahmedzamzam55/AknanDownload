use super::HlsJob;
use crate::http::{request, Progress, SendOrdered, Shared};
use anyhow::{anyhow, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub(super) fn decrypt(key: &[u8], iv: &[u8; 16], data: &mut Vec<u8>) -> anyhow::Result<()> {
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let dec = cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
        .map_err(|_| anyhow!("invalid AES key length"))?;
    let len = dec
        .decrypt_padded_mut::<Pkcs7>(data)
        .map_err(|_| anyhow!("could not decrypt stream part"))?
        .len();
    data.truncate(len);
    Ok(())
}

/// One address of the stream (a list, a key, a part), or only `range`
/// (offset, length) of it. Also gives the address it came from in the end
/// (after redirects): what the list's own addresses are relative to.
pub(super) async fn fetch_full(shared: &Shared, job: &HlsJob, url: &str, range: Option<(u64, u64)>) -> anyhow::Result<(Vec<u8>, String)> {
    let mut rb = request(&crate::http::client_for(&shared.client(), url), url, &job.headers, &job.user_agent);
    if let Some((off, len)) = range {
        rb = rb.header(reqwest::header::RANGE, format!("bytes={}-{}", off, off + len.max(1) - 1));
    }
    let resp = rb.send_ordered().await?;
    if !resp.status().is_success() {
        return Err(crate::http::HttpStatus(resp.status().as_u16()).into());
    }
    let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let final_url = resp.url().to_string();
    let mut data = resp.bytes().await?.to_vec();
    if let Some((off, len)) = range {
        if !partial {
            // the server sent all of it: cut the part out ourselves
            let (a, b) = (off as usize, (off + len) as usize);
            if data.len() < b {
                bail!("a part of the stream is shorter than its list says");
            }
            data = data[a..b].to_vec();
        } else if data.len() as u64 != len {
            return Err(crate::http::NetDrop("a part of the stream came incomplete").into());
        }
    }
    Ok((data, final_url))
}

pub(super) async fn fetch_bytes(shared: &Shared, job: &HlsJob, url: &str) -> anyhow::Result<Vec<u8>> {
    Ok(fetch_full(shared, job, url, None).await?.0)
}

/// A playlist, and the address it really came from.
pub(super) async fn fetch_text(shared: &Shared, job: &HlsJob, url: &str) -> anyhow::Result<(String, String)> {
    let (b, at) = fetch_full(shared, job, url, None).await?;
    Ok((String::from_utf8_lossy(&b).into_owned(), at))
}

pub(super) fn part_file(dir: &Path, i: usize) -> PathBuf {
    dir.join(format!("{i:06}.part"))
}

pub(super) async fn join_parts(
    shared: &Shared,
    job: &HlsJob,
    progress: &Progress,
    init_url: Option<&str>,
    init_range: Option<(u64, u64)>,
    chosen: Arc<Vec<usize>>,
) -> anyhow::Result<()> {
    progress.assembling.store(true, Ordering::Relaxed);
    let init = match init_url {
        Some(u) => Some(fetch_full(shared, job, u, init_range).await?.0),
        None => None,
    };
    let temp_dir = job.temp_dir.clone();
    let output = job.output.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&output)?);
        if let Some(init) = init {
            out.write_all(&init)?;
        }
        for &i in chosen.iter() {
            let mut f = std::fs::File::open(part_file(&temp_dir, i))?;
            std::io::copy(&mut f, &mut out)?;
        }
        out.flush()?;
        drop(out);
        let _ = std::fs::remove_dir_all(&temp_dir);
        Ok(())
    })
    .await??;
    Ok(())
}
