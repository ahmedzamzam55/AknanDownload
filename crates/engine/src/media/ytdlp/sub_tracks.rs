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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

const PREF_EXTS: &[&str] = &["srt", "vtt", "srv3", "ttml", "ass", "ssa", "bcc", "srv1", "json3", "m3u8"];

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

fn headers_of(item: &Value) -> Vec<(String, String)> {
    item["http_headers"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default()
}

type PickedSub<'a> = (&'a str, &'a str, Option<&'a str>, Vec<(String, String)>);

fn pick_format<'a>(arr: &'a [Value]) -> Option<PickedSub<'a>> {
    let valid = |item: &'a Value| -> Option<PickedSub<'a>> {
        let url = item["url"].as_str()?;
        let ext = item["ext"].as_str().unwrap_or("vtt");
        if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")) {
            return None;
        }
        Some((url, ext, item["name"].as_str(), headers_of(item)))
    };
    for &wanted in PREF_EXTS {
        if let Some(found) = arr.iter().filter_map(valid).find(|(_, ext, _, _)| ext.eq_ignore_ascii_case(wanted)) {
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
        let Some((url, ext, raw_name, headers)) = pick_format(arr) else { continue };
        let norm_lang = lang.strip_suffix("-orig").unwrap_or(lang);
        if !seen.insert(norm_lang.to_string()) {
            continue;
        }
        out.push(YtSubtitle {
            lang: norm_lang.to_string(),
            name: clean_name(norm_lang, raw_name),
            url: url.to_string(),
            ext: ext.to_ascii_lowercase(),
            auto,
            headers,
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
