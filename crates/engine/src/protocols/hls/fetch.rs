use super::prep::decrypt_if_needed;
use super::sanitize::{detect_container, parse_retry_after, scoped_headers, strip_ts_preamble, HlsContainer};
use super::HlsJob;
use crate::http::{request, HttpRetry, HttpStatus, NetDrop, Progress, SendOrdered, Shared};
use crate::m3u8::Part;
use anyhow::{anyhow, bail};
use std::collections::HashMap;
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
            self.progress.hls_workers[self.slot % 32].0.fetch_sub(self.added, Ordering::Relaxed);
        }
    }
}

fn adjust_progress_bytes(progress: &Progress, slot: usize, added: &mut u64, final_len: u64) {
    if final_len < *added {
        let d = *added - final_len;
        progress.bytes.fetch_sub(d, Ordering::Relaxed);
        progress.hls_workers[slot].0.fetch_sub(d, Ordering::Relaxed);
    } else if final_len > *added {
        let d = final_len - *added;
        progress.bytes.fetch_add(d, Ordering::Relaxed);
        progress.hls_workers[slot].0.fetch_add(d, Ordering::Relaxed);
    }
    *added = final_len;
}

pub(super) fn decrypt(key: &[u8], iv: &[u8; 16], data: &mut Vec<u8>) -> anyhow::Result<()> {
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let dec = cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
        .map_err(|_| anyhow!("invalid AES key length"))?;
    let len = dec.decrypt_padded_mut::<Pkcs7>(data).map_err(|_| anyhow!("could not decrypt stream part"))?.len();
    data.truncate(len);
    Ok(())
}

pub(super) async fn fetch_full(
    shared: &Shared,
    job: &HlsJob,
    url: &str,
    range: Option<(u64, u64)>,
) -> anyhow::Result<(Vec<u8>, String)> {
    let headers = scoped_headers(&job.headers, &job.url, url);
    let mut rb = request(&crate::http::client_for(&shared.client(), url), url, &headers, &job.user_agent);
    if let Some((off, len)) = range {
        rb = rb.header(reqwest::header::RANGE, format!("bytes={}-{}", off, off + len.max(1) - 1));
    }
    let resp = rb.send_ordered().await?;
    if !resp.status().is_success() {
        return Err(HttpStatus(resp.status().as_u16()).into());
    }
    let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let final_url = resp.url().to_string();
    let mut data = resp.bytes().await?.to_vec();
    if let Some((off, len)) = range {
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
    Ok((data, final_url))
}

pub(super) async fn fetch_text(shared: &Shared, job: &HlsJob, url: &str) -> anyhow::Result<(String, String)> {
    let (b, at) = fetch_full(shared, job, url, None).await?;
    Ok((String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').into(), at))
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
    peak_ok: &AtomicUsize,
    slot: usize,
    part: &Part,
    key: Option<&[u8]>,
    path: &Path,
) -> anyhow::Result<u64> {
    let slot = slot % 32;
    let headers = scoped_headers(&job.headers, &job.url, &part.url);
    let mut rb = request(&crate::http::client_for(&shared.client(), &part.url), &part.url, &headers, &job.user_agent);
    if let Some((off, len)) = part.range {
        rb = rb.header(reqwest::header::RANGE, format!("bytes={}-{}", off, off + len.max(1) - 1));
    }
    let mut resp = rb.send_ordered().await?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let ra = resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(parse_retry_after);
        return Err(HttpRetry(code, ra).into());
    }
    let cur_ok = ok_active.fetch_add(1, Ordering::Relaxed) + 1;
    peak_ok.fetch_max(cur_ok, Ordering::Relaxed);
    let _ok = OkGuard(ok_active);
    let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let expected_len = (!partial && part.range.is_none()).then(|| resp.content_length()).flatten();
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
    if expected_len.is_some_and(|exp| exp > 0 && (data.len() as u64) < exp) {
        return Err(NetDrop("a part of the stream came incomplete").into());
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
    if detect_container(&data) == HlsContainer::MpegTs {
        let stripped = strip_ts_preamble(&data);
        if stripped.len() < data.len() {
            data = stripped.to_vec();
        }
    }
    let final_len = data.len() as u64;
    adjust_progress_bytes(progress, slot, &mut rb_guard.added, final_len);
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

type InitCache = HashMap<(String, Option<(u64, u64)>), Vec<u8>>;

pub(super) async fn join_parts(
    shared: &Shared,
    job: &HlsJob,
    progress: &Progress,
    parts: Arc<Vec<Part>>,
    keys: Arc<HashMap<String, Vec<u8>>>,
    chosen: Arc<Vec<usize>>,
) -> anyhow::Result<HlsContainer> {
    progress.assembling.store(true, Ordering::Relaxed);
    let mut init_cache: InitCache = HashMap::new();
    let mut seq_inits: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut last_key: Option<(String, Option<(u64, u64)>)> = None;
    let mut container = HlsContainer::MpegTs;
    for &idx in chosen.iter() {
        let p = &parts[idx];
        let Some(u) = p.init_url.clone() else { continue };
        let k = (u.clone(), p.init_range);
        if last_key.as_ref() != Some(&k) {
            if !init_cache.contains_key(&k) {
                let mut bytes = fetch_full(shared, job, &u, p.init_range).await?.0;
                decrypt_if_needed(p.init_key.as_ref(), &keys, 0, &mut bytes)?;
                init_cache.insert(k.clone(), bytes);
            }
            if let Some(b) = init_cache.get(&k) {
                if detect_container(b) == HlsContainer::Fmp4 {
                    container = HlsContainer::Fmp4;
                }
                seq_inits.push((idx, b.clone()));
            }
            last_key = Some(k);
        }
    }
    let (temp_dir, output) = (job.temp_dir.clone(), job.output.clone());
    let detected = tokio::task::spawn_blocking(move || -> anyhow::Result<HlsContainer> {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&output)?);
        let mut init_it = seq_inits.into_iter().peekable();
        let mut kind = container;
        let mut head_checked = false;
        for &i in chosen.iter() {
            if let Some((_, init)) = init_it.next_if(|(at, _)| *at == i) {
                out.write_all(&init)?;
            }
            let part_path = part_file(&temp_dir, i);
            if !head_checked {
                use std::io::Read;
                let mut buf = [0u8; 32];
                if let Ok(n) = std::fs::File::open(&part_path).and_then(|mut f| f.read(&mut buf)) {
                    if detect_container(&buf[..n]) == HlsContainer::Fmp4 {
                        kind = HlsContainer::Fmp4;
                    }
                }
                head_checked = true;
            }
            let mut f = std::fs::File::open(part_path)?;
            std::io::copy(&mut f, &mut out)?;
        }
        out.flush()?;
        let _ = std::fs::write(temp_dir.join(".joined"), b"1");
        Ok(kind)
    })
    .await??;
    Ok(detected)
}
