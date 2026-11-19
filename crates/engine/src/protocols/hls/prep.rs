use super::fetch::{fetch_full, fetch_text};
use super::sanitize::scoped_headers;
use super::HlsJob;
use crate::http::{http_status_of, Shared};
use crate::http_download::Cancelled;
use crate::m3u8::{self, parse_media, Key, MediaPlaylist};
use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Deserialize, PartialEq)]
struct PlaylistMeta {
    url_key: String,
    part_count: usize,
}

fn stable_url_key(url: &str) -> String {
    url::Url::parse(url)
        .map(|mut u| {
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        })
        .unwrap_or_else(|_| url.to_string())
}

pub(super) async fn load_playlist(
    shared: &Shared,
    job: &HlsJob,
) -> anyhow::Result<(MediaPlaylist, String, Option<String>)> {
    let (mut text, mut url) = match job.initial_playlist.as_deref() {
        Some(raw) if !raw.trim().is_empty() => (raw.trim_start_matches('\u{feff}').to_string(), job.url.clone()),
        _ => fetch_text(shared, job, &job.url).await.context("could not load playlist")?,
    };
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with("#EXTM3U") {
        bail!("not a valid HLS playlist");
    }
    let mut audio_url = None;
    if trimmed.contains("#EXT-X-STREAM-INF") {
        let v = m3u8::best_variant_info(&url, trimmed)?.ok_or_else(|| anyhow!("no video variant found"))?;
        audio_url = v.audio_url;
        (text, url) = fetch_text(shared, job, &v.url).await?;
    }
    let pl = parse_media(&url, text.trim_start_matches('\u{feff}'))?;
    if !pl.ended {
        bail!("live streams are not supported (the playlist has no end)");
    }
    if pl.parts.is_empty() {
        bail!("the playlist has no parts");
    }
    Ok((pl, url, audio_url))
}

fn decode_base64(input: &str) -> anyhow::Result<Vec<u8>> {
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let val = |b: u8| -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut buf = 0u32;
        for (idx, &b) in chunk.iter().enumerate() {
            buf |= val(b).ok_or_else(|| anyhow!("invalid base64 key byte"))? << (18 - idx * 6);
        }
        out.push((buf >> 16) as u8);
        if chunk.len() >= 3 {
            out.push((buf >> 8) as u8);
        }
        if chunk.len() >= 4 {
            out.push(buf as u8);
        }
    }
    Ok(out)
}

fn decode_data_key(uri: &str) -> anyhow::Result<Vec<u8>> {
    let body = uri.strip_prefix("data:").ok_or_else(|| anyhow!("not a data key"))?;
    let (meta, data) = body.split_once(',').ok_or_else(|| anyhow!("invalid data URI key"))?;
    if meta.to_ascii_lowercase().contains(";base64") {
        decode_base64(data)
    } else {
        Ok(percent_encoding::percent_decode_str(data).collect())
    }
}

pub(super) async fn fetch_key_bytes(
    shared: &Shared,
    job: &HlsJob,
    uri: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<Vec<u8>> {
    if uri.starts_with("data:") {
        return decode_data_key(uri);
    }
    let mut scoped = job.clone();
    scoped.headers = scoped_headers(&job.headers, &job.url, uri);
    let max_tries = job.retries.clamp(1, 4);
    let mut last_err = None;
    for attempt in 0..max_tries {
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let res = tokio::select! {
            r = fetch_full(shared, &scoped, uri, None) => r,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        match res {
            Ok((b, _)) if !b.is_empty() => return Ok(b),
            Ok(_) => last_err = Some(anyhow!("empty AES-128 key")),
            Err(e) => {
                if matches!(http_status_of(&e), Some(401 | 403 | 404 | 410)) {
                    return Err(e).context("could not load key");
                }
                last_err = Some(e);
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(attempt as u64 + 1)) => {}
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("could not load key"))).context("could not load key")
}

pub(super) fn decrypt_if_needed(
    key: Option<&Key>,
    keys: &HashMap<String, Vec<u8>>,
    seq: u64,
    data: &mut Vec<u8>,
) -> anyhow::Result<()> {
    let Some(k) = key else { return Ok(()) };
    let kb = keys.get(&k.uri).ok_or_else(|| anyhow!("missing key"))?;
    let iv = k.iv.unwrap_or_else(|| (seq as u128).to_be_bytes());
    super::fetch::decrypt(kb, &iv, data)
}

pub(super) fn prepare_resume_dir(dir: &Path, playlist_url: &str, part_count: usize) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let meta_path = dir.join(".meta.json");
    let current = PlaylistMeta { url_key: stable_url_key(playlist_url), part_count };
    let matches = std::fs::read(&meta_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<PlaylistMeta>(&b).ok())
        .is_some_and(|prev| prev == current);
    if !matches {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
                if matches!(ext, "part" | "tmp") || p.file_name().and_then(|s| s.to_str()) == Some(".joined") {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        let _ = std::fs::write(&meta_path, serde_json::to_vec(&current).unwrap_or_default());
    } else if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("part") && e.metadata().map(|m| m.len() == 0).unwrap_or(true) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    Ok(())
}
