//! Parsing of HLS playlists (.m3u8).

use anyhow::{anyhow, bail};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub uri: String,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub url: String,
    pub sequence: u64,
    pub key: Option<Key>,
    /// only these bytes of the address (#EXT-X-BYTERANGE): (offset, length)
    pub range: Option<(u64, u64)>,
    /// how long it plays, in seconds (#EXTINF)
    pub duration: f64,
}

#[derive(Debug, Default, PartialEq)]
pub struct MediaPlaylist {
    pub init_url: Option<String>,
    /// only these bytes of the init address: (offset, length)
    pub init_range: Option<(u64, u64)>,
    pub parts: Vec<Part>,
    pub ended: bool,
}

/// Parse `KEY=VALUE,KEY="quoted,value"` attribute lists.
pub fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut key = String::new();
    let mut val = String::new();
    let mut in_key = true;
    let mut quoted = false;
    for c in s.chars() {
        if in_key {
            if c == '=' {
                in_key = false;
            } else if c != ',' {
                key.push(c);
            }
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ',' && !quoted {
            out.insert(key.trim().to_ascii_uppercase(), val.clone());
            key.clear();
            val.clear();
            in_key = true;
        } else {
            val.push(c);
        }
    }
    if !key.trim().is_empty() {
        out.insert(key.trim().to_ascii_uppercase(), val);
    }
    out
}

pub(super) fn resolve(base: &str, rel: &str) -> anyhow::Result<String> {
    let base = url::Url::parse(base)?;
    Ok(base.join(rel.trim())?.to_string())
}

mod master;
pub use master::{parse_master, Variant};

/// For a master playlist, return the URL of the highest bandwidth variant.
pub fn best_variant(base: &str, text: &str) -> anyhow::Result<Option<String>> {
    Ok(parse_master(base, text)?.0.into_iter().max_by_key(|v| v.bandwidth).map(|v| v.url))
}

/// "1000@200" -> (1000, Some(200)); "1000" -> (1000, None)
fn parse_byterange(s: &str) -> Option<(u64, Option<u64>)> {
    let s = s.trim().trim_matches('"');
    match s.split_once('@') {
        Some((len, off)) => Some((len.trim().parse().ok()?, Some(off.trim().parse().ok()?))),
        None => Some((s.parse().ok()?, None)),
    }
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let hex = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A real stream has a few thousand parts; a list far beyond this is an
/// attack on memory and disk, not a video.
pub const MAX_PARTS: usize = 200_000;
pub const MAX_PLAYLIST_BYTES: usize = 32 << 20;

impl MediaPlaylist {
    /// How long the whole stream plays (seconds).
    pub fn duration(&self) -> f64 {
        self.parts.iter().map(|p| p.duration).sum()
    }

    /// The parts that play between `start` and `end` (seconds): their
    /// numbers, and when the first of them starts.
    pub fn parts_between(&self, start: f64, end: f64) -> (Vec<usize>, f64) {
        let mut at = 0.0;
        let mut chosen = Vec::new();
        let mut first_at = None;
        for (i, p) in self.parts.iter().enumerate() {
            let (a, b) = (at, at + p.duration);
            at = b;
            if b > start && a < end {
                first_at.get_or_insert(a);
                chosen.push(i);
            }
        }
        (chosen, first_at.unwrap_or(0.0))
    }
}

pub fn parse_media(base: &str, text: &str) -> anyhow::Result<MediaPlaylist> {
    if text.len() > MAX_PLAYLIST_BYTES {
        bail!("the stream's list is too big");
    }
    let mut pl = MediaPlaylist::default();
    let mut seq: u64 = 0;
    let mut key: Option<Key> = None;
    // #EXT-X-BYTERANGE for the next part, and where the last range of each
    // address ended (a range without "@offset" goes on from there)
    let mut next_range: Option<(u64, Option<u64>)> = None;
    let mut range_end: HashMap<String, u64> = HashMap::new();
    let mut next_duration = 0.0f64;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let a = parse_attrs(attrs);
            let method = a.get("METHOD").map(|s| s.to_ascii_uppercase()).unwrap_or_default();
            key = match method.as_str() {
                "NONE" | "" => None,
                "AES-128" => {
                    let uri = a.get("URI").ok_or_else(|| anyhow!("AES-128 key without URI"))?;
                    Some(Key {
                        uri: resolve(base, uri)?,
                        iv: a.get("IV").and_then(|v| parse_iv(v)),
                    })
                }
                other => bail!("this stream is DRM protected ({other}) and cannot be downloaded"),
            };
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            let a = parse_attrs(attrs);
            if let Some(uri) = a.get("URI") {
                pl.init_url = Some(resolve(base, uri)?);
                pl.init_range = a.get("BYTERANGE").and_then(|r| parse_byterange(r)).map(|(len, off)| (off.unwrap_or(0), len));
            }
        } else if let Some(v) = line.strip_prefix("#EXTINF:") {
            let n = v.split(',').next().unwrap_or("").trim();
            next_duration = n.parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0 && *d < 86_400.0).unwrap_or(0.0);
        } else if let Some(v) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            next_range = Some(parse_byterange(v).ok_or_else(|| anyhow!("the stream's list has a broken byte range"))?);
        } else if line.starts_with("#EXT-X-ENDLIST") {
            pl.ended = true;
        } else if !line.starts_with('#') {
            if pl.parts.len() >= MAX_PARTS {
                bail!("the stream has too many parts");
            }
            let url = resolve(base, line)?;
            let range = next_range.take().map(|(len, off)| {
                let start = off.unwrap_or_else(|| range_end.get(&url).copied().unwrap_or(0));
                range_end.insert(url.clone(), start + len);
                (start, len)
            });
            pl.parts.push(Part {
                url,
                sequence: seq,
                key: key.clone(),
                range,
                duration: std::mem::take(&mut next_duration),
            });
            seq = seq.wrapping_add(1);
        }
    }
    Ok(pl)
}

#[cfg(test)]
mod tests;
