use super::{parse_attrs, resolve};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: String,
    pub width: u64,
    pub height: u64,
    pub bandwidth: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsSub {
    pub lang: String,
    pub name: String,
    pub url: String,
}

/// Parse `#EXT-X-MEDIA:TYPE=SUBTITLES` entries with a `URI` attribute.
pub fn parse_subtitles_media(base: &str, text: &str) -> Vec<HlsSub> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") else { continue };
        let a = parse_attrs(attrs);
        if !a.get("TYPE").is_some_and(|t| t.eq_ignore_ascii_case("SUBTITLES")) {
            continue;
        }
        let Some(uri) = a.get("URI").filter(|u| !u.is_empty()) else { continue };
        let Ok(url) = resolve(base, uri) else { continue };
        let lang = a.get("LANGUAGE").cloned().unwrap_or_else(|| "und".into());
        let name = a.get("NAME").cloned().unwrap_or_else(|| lang.clone());
        out.push(HlsSub { lang, name, url });
    }
    out
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
