use super::{parse_attrs, resolve};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: String,
    pub width: u64,
    pub height: u64,
    pub bandwidth: u64,
}

/// Parse a master `#EXTM3U` playlist into sorted, deduplicated quality variants
/// and report whether audio comes in separate `#EXT-X-MEDIA` playlists.
pub fn parse_master(base: &str, text: &str) -> anyhow::Result<(Vec<Variant>, bool)> {
    if !text.lines().any(|l| l.trim().starts_with("#EXT-X-STREAM-INF")) {
        return Ok((Vec::new(), false));
    }
    let mut variants = Vec::new();
    let mut separate_audio = false;
    let mut pending: Option<(u64, u64, u64)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = parse_attrs(attrs);
            if a.get("TYPE").is_some_and(|t| t.eq_ignore_ascii_case("AUDIO")) && a.contains_key("URI") {
                separate_audio = true;
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let a = parse_attrs(attrs);
            let bw = a.get("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
            let (w, h) = a
                .get("RESOLUTION")
                .and_then(|r| r.split_once('x'))
                .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
                .unwrap_or((0, 0));
            pending = Some((w, h, bw));
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some((width, height, bandwidth)) = pending.take() {
                if let Ok(url) = resolve(base, line) {
                    variants.push(Variant { url, width, height, bandwidth });
                }
            }
        }
    }
    variants.sort_by(|a, b| b.height.cmp(&a.height).then(b.bandwidth.cmp(&a.bandwidth)));
    let mut seen = HashSet::new();
    variants.retain(|v| {
        let key = if v.height > 0 { v.height } else { u64::MAX - v.bandwidth.min(u64::MAX - 1) };
        seen.insert(key)
    });
    Ok((variants, separate_audio))
}
