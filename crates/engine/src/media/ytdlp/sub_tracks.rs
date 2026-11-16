use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

/// One subtitle / closed-caption track extracted from a video page or HLS playlist.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YtSubtitle {
    pub lang: String,
    pub name: String,
    pub url: String,
    pub ext: String,
    pub auto: bool,
}

const PREF_EXTS: &[&str] = &["srt", "vtt", "srv3", "ttml", "srv1", "json3"];
const AUTO_KEEP: &[&str] = &[
    "ar", "en", "fr", "de", "es", "pt", "tr", "ru", "it", "ja", "ko", "zh-Hans", "zh", "hi", "id", "fa", "ur",
];

fn clean_name(lang: &str, raw_name: Option<&str>) -> String {
    let base = lang.split(['-', '_']).next().unwrap_or(lang);
    let known = match lang {
        "zh-Hans" => Some("Chinese"),
        _ => match base {
            "ar" => Some("Arabic"),
            "en" => Some("English"),
            "fr" => Some("French"),
            "de" => Some("German"),
            "es" => Some("Spanish"),
            "pt" => Some("Portuguese"),
            "tr" => Some("Turkish"),
            "ru" => Some("Russian"),
            "it" => Some("Italian"),
            "ja" => Some("Japanese"),
            "ko" => Some("Korean"),
            "zh" => Some("Chinese"),
            "hi" => Some("Hindi"),
            "id" => Some("Indonesian"),
            "fa" => Some("Persian"),
            "ur" => Some("Urdu"),
            _ => None,
        },
    };
    if let Some(n) = known {
        return n.to_string();
    }
    raw_name
        .map(|s| s.replace("(auto-generated)", "").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| lang.to_string())
}

fn pick_format<'a>(arr: &'a [Value]) -> Option<(&'a str, &'a str, Option<&'a str>)> {
    let valid = |item: &'a Value| -> Option<(&'a str, &'a str, Option<&'a str>)> {
        let url = item["url"].as_str()?;
        let ext = item["ext"].as_str().unwrap_or("vtt");
        let protocol = item["protocol"].as_str().unwrap_or("");
        if !(url.starts_with("http://") || url.starts_with("https://")) || protocol.starts_with("m3u8") || ext == "m3u8" {
            return None;
        }
        Some((url, ext, item["name"].as_str()))
    };
    for &wanted in PREF_EXTS {
        if let Some(found) = arr.iter().filter_map(valid).find(|(_, ext, _)| ext.eq_ignore_ascii_case(wanted)) {
            return Some(found);
        }
    }
    arr.iter().filter_map(valid).next()
}

fn collect_map(map: &serde_json::Map<String, Value>, auto: bool, seen: &mut HashSet<String>, out: &mut Vec<YtSubtitle>) {
    for (lang, list) in map {
        if lang.eq_ignore_ascii_case("live_chat") {
            continue;
        }
        let Some(arr) = list.as_array() else { continue };
        let Some((url, ext, raw_name)) = pick_format(arr) else { continue };
        let norm_lang = lang.strip_suffix("-orig").unwrap_or(lang);
        let base = norm_lang.split(['-', '_']).next().unwrap_or(norm_lang);
        if auto {
            let is_orig = lang.ends_with("-orig") || !url.contains("tlang=");
            if !(is_orig || AUTO_KEEP.contains(&norm_lang) || AUTO_KEEP.contains(&base)) {
                continue;
            }
            if seen.contains(norm_lang) || seen.contains(base) {
                continue;
            }
        }
        if !seen.insert(norm_lang.to_string()) {
            continue;
        }
        out.push(YtSubtitle {
            lang: norm_lang.to_string(),
            name: clean_name(norm_lang, raw_name),
            url: url.to_string(),
            ext: ext.to_ascii_lowercase(),
            auto,
        });
    }
}

/// Extract manual and curated automatic subtitle tracks from yt-dlp's JSON metadata.
pub(super) fn parse_subtitles(info: &Value) -> Vec<YtSubtitle> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(m) = info["subtitles"].as_object() {
        collect_map(m, false, &mut seen, &mut out);
    }
    if let Some(m) = info["automatic_captions"].as_object() {
        collect_map(m, true, &mut seen, &mut out);
    }
    fn rank(s: &YtSubtitle) -> (bool, u8, &str, &str) {
        let base = s.lang.split(['-', '_']).next().unwrap_or(&s.lang);
        let top = match base {
            "ar" => 0,
            "en" => 1,
            _ => 2,
        };
        (s.auto, top, s.name.as_str(), s.lang.as_str())
    }
    out.sort_by(|a, b| rank(a).cmp(&rank(b)));
    out
}
