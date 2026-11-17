use super::HlsJob;
use crate::http::{request, HttpStatus, NetDrop, Progress, SendOrdered, Shared};
use crate::m3u8::Part;
use anyhow::{anyhow, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct OkGuard<'a>(&'a AtomicUsize);
impl Drop for OkGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct ByteRollback<'a> {
    progress: &'a Progress,
    slot: usize,
    added: u64,
    committed: bool,
}
impl Drop for ByteRollback<'_> {
    fn drop(&mut self) {
        if !self.committed && self.added > 0 {
            self.progress.bytes.fetch_sub(self.added, Ordering::Relaxed);
            self.progress.hls_workers[self.slot].0.fetch_sub(self.added, Ordering::Relaxed);
        }
    }
}

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

#[allow(clippy::too_many_arguments)]
pub(super) async fn fetch_part_stream(
    shared: &Shared,
    job: &HlsJob,
    progress: &Progress,
    ok_active: &AtomicUsize,
    slot: usize,
    part: &Part,
    key: Option<&[u8]>,
    path: &Path,
) -> anyhow::Result<u64> {
    let mut rb = request(&crate::http::client_for(&shared.client(), &part.url), &part.url, &job.headers, &job.user_agent);
    if let Some((off, len)) = part.range {
        rb = rb.header(reqwest::header::RANGE, format!("bytes={}-{}", off, off + len.max(1) - 1));
    }
    let mut resp = rb.send_ordered().await?;
    if !resp.status().is_success() {
        return Err(HttpStatus(resp.status().as_u16()).into());
    }
    ok_active.fetch_add(1, Ordering::Relaxed);
    let _ok = OkGuard(ok_active);
    let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let cap = part.range.map(|(_, l)| l as usize).unwrap_or_else(|| resp.content_length().unwrap_or(0) as usize).min(8 << 20);
    let mut data = Vec::with_capacity(cap);
    let mut rb_guard = ByteRollback { progress, slot, added: 0, committed: false };
    while let Some(chunk) = resp.chunk().await? {
        let n = chunk.len();
        shared.limiter.acquire(n).await;
        job.limiter.acquire(n).await;
        progress.online();
        data.extend_from_slice(&chunk);
        let counted = if let Some((_, want_len)) = part.range {
            want_len.saturating_sub(rb_guard.added).min(n as u64)
        } else {
            n as u64
        };
        if counted > 0 {
            progress.bytes.fetch_add(counted, Ordering::Relaxed);
            progress.hls_workers[slot].0.fetch_add(counted, Ordering::Relaxed);
            rb_guard.added += counted;
        }
    }
    if let Some((off, len)) = part.range {
        if !partial {
            let (a, b) = (off as usize, (off + len) as usize);
            if data.len() < b {
                bail!("a part of the stream is shorter than its list says");
            }
            data = data[a..b].to_vec();
        } else if data.len() as u64 != len {
            return Err(NetDrop("a part of the stream came incomplete").into());
        }
    }
    if let (Some(k), Some(key_bytes)) = (&part.key, key) {
        let iv = k.iv.unwrap_or_else(|| (part.sequence as u128).to_be_bytes());
        decrypt(key_bytes, &iv, &mut data)?;
    }
    let final_len = data.len() as u64;
    if final_len < rb_guard.added {
        let diff = rb_guard.added - final_len;
        progress.bytes.fetch_sub(diff, Ordering::Relaxed);
        progress.hls_workers[slot].0.fetch_sub(diff, Ordering::Relaxed);
        rb_guard.added = final_len;
    } else if final_len > rb_guard.added {
        let diff = final_len - rb_guard.added;
        progress.bytes.fetch_add(diff, Ordering::Relaxed);
        progress.hls_workers[slot].0.fetch_add(diff, Ordering::Relaxed);
        rb_guard.added = final_len;
    }
    // on the disk before it counts as there (a power cut left empty pieces that were taken as done)
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&data)?;
        f.sync_data()?;
    }
    std::fs::rename(&tmp, path)?;
    rb_guard.committed = true;
    Ok(final_len)
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
