use super::{parse_attrs, resolve};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: String,
    pub width: u64,
    pub height: u64,
    pub bandwidth: u64,
    pub audio_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsSub {
    pub lang: String,
    pub name: String,
    pub url: String,
}

#[derive(Clone)]
struct AudioGroupPick {
    url: String,
    score: u8,
}

/// Parse `#EXT-X-MEDIA:TYPE=SUBTITLES` entries with a `URI` attribute.
pub fn parse_subtitles_media(base: &str, text: &str) -> Vec<HlsSub> {
    let text = text.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    for line in text.lines().map(|l| l.trim().trim_start_matches('\u{feff}')) {
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

/// Parse a master `#EXTM3U` playlist into sorted, deduplicated quality variants,
/// attaching separate `#EXT-X-MEDIA:TYPE=AUDIO` URLs by `GROUP-ID` when present.
pub fn parse_master(base: &str, text: &str) -> anyhow::Result<(Vec<Variant>, bool)> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.lines().any(|l| l.trim().starts_with("#EXT-X-STREAM-INF")) {
        return Ok((Vec::new(), false));
    }
    let mut audio_groups: HashMap<String, AudioGroupPick> = HashMap::new();
    let mut default_audio: Option<String> = None;
    let mut separate_audio = false;
    let mut pending: Option<(u64, u64, u64, Option<String>)> = None;
    let mut raw_variants: Vec<(u64, u64, u64, Option<String>, String)> = Vec::new();

    for line in text.lines().map(|l| l.trim().trim_start_matches('\u{feff}')) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = parse_attrs(attrs);
            if a.get("TYPE").is_some_and(|t| t.eq_ignore_ascii_case("AUDIO")) {
                if let Some(uri) = a.get("URI").filter(|u| !u.is_empty()) {
                    separate_audio = true;
                    if let Ok(resolved) = resolve(base, uri) {
                        let gid = a.get("GROUP-ID").cloned().unwrap_or_default();
                        let score = if a.get("DEFAULT").is_some_and(|v| v.eq_ignore_ascii_case("YES")) {
                            2
                        } else if a.get("AUTOSELECT").is_some_and(|v| v.eq_ignore_ascii_case("YES")) {
                            1
                        } else {
                            0
                        };
                        if default_audio.is_none() || score == 2 {
                            default_audio = Some(resolved.clone());
                        }
                        let replace = audio_groups.get(&gid).is_none_or(|cur| score > cur.score);
                        if replace {
                            audio_groups.insert(gid, AudioGroupPick { url: resolved, score });
                        }
                    }
                }
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let a = parse_attrs(attrs);
            let bw = a.get("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
            let (w, h) = a
                .get("RESOLUTION")
                .and_then(|r| r.split_once('x'))
                .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
                .unwrap_or((0, 0));
            let audio_gid = a.get("AUDIO").cloned();
            pending = Some((w, h, bw, audio_gid));
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some((width, height, bandwidth, audio_gid)) = pending.take() {
                if let Ok(url) = resolve(base, line) {
                    raw_variants.push((width, height, bandwidth, audio_gid, url));
                }
            }
        }
    }

    let mut variants: Vec<Variant> = raw_variants
        .into_iter()
        .map(|(width, height, bandwidth, gid, url)| {
            let audio_url = gid
                .as_ref()
                .and_then(|g| audio_groups.get(g).map(|p| p.url.clone()))
                .or_else(|| if separate_audio { default_audio.clone() } else { None });
            Variant { url, width, height, bandwidth, audio_url }
        })
        .collect();

    variants.sort_by(|a, b| b.height.cmp(&a.height).then(b.bandwidth.cmp(&a.bandwidth)));
    let mut seen = HashSet::new();
    variants.retain(|v| {
        let key = if v.height > 0 { v.height } else { u64::MAX - v.bandwidth.min(u64::MAX - 1) };
        seen.insert(key)
    });
    Ok((variants, separate_audio))
}
