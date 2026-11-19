use super::extract_extra::{base_href, decode_html_entities, decode_json_url, push_extra_links};
use super::GrabOptions;
use crate::config::wildcard_match;
use crate::filename;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

const PAGE_EXTS: [&str; 13] =
    ["", "html", "htm", "php", "asp", "aspx", "jsp", "shtml", "cfm", "cgi", "pl", "xhtml", "phtml"];

pub fn is_page_ext(ext: &str) -> bool {
    PAGE_EXTS.contains(&ext)
}

fn anchor_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)<a\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>(.*?)</a\s*>"#).unwrap()
    })
}

fn attr_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:src|href|data-src|data-href|poster)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
    })
}

fn tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap())
}

fn decode_entities(s: &str) -> String {
    decode_html_entities(s)
}

/// Every link of an HTML page as (absolute address, link text).
pub fn extract_links(html: &str, base: &str) -> Vec<(String, String)> {
    let Ok(page_url) = url::Url::parse(base) else { return Vec::new() };
    let base = base_href(html, &page_url);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |raw: &str, text: String| {
        let raw = decode_entities(raw.trim());
        if raw.is_empty()
            || raw.starts_with('#')
            || ["javascript:", "mailto:", "data:", "tel:", "about:"]
                .iter()
                .any(|p| raw.to_ascii_lowercase().starts_with(p))
        {
            return;
        }
        let Ok(mut u) = base.join(&raw) else { return };
        if !matches!(u.scheme(), "http" | "https") {
            return;
        }
        u.set_fragment(None);
        let s = u.to_string();
        if seen.insert(s.clone()) {
            out.push((s, text));
        }
    };
    for c in anchor_re().captures_iter(html) {
        let href = c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str()).unwrap_or("");
        let text = c.get(4).map(|m| tag_re().replace_all(m.as_str(), " ").to_string()).unwrap_or_default();
        let text = decode_entities(&text).split_whitespace().collect::<Vec<_>>().join(" ");
        push(href, text);
    }
    for c in attr_re().captures_iter(html) {
        let v = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
        push(v, String::new());
    }
    // addresses written as text (lists of links in a post, a note, JSON in
    // a script): many pages have their file links only like that
    for m in text_url_re().find_iter(html) {
        let u = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?', '\\']);
        push(&decode_json_url(u), String::new());
    }
    push_extra_links(html, &mut push);
    out
}

fn text_url_re() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r#"https?:(?:\\?/){2}[^\s"'<>()\[\]{}|^`]+"#).unwrap())
}

pub(super) fn ext_of(u: &url::Url) -> String {
    let last = u.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    let last = percent_encoding::percent_decode_str(last).decode_utf8_lossy().to_string();
    filename::extension(&last)
}

pub(super) fn wanted_ext(opts: &GrabOptions, ext: &str) -> bool {
    if opts.exts.is_empty() {
        return !is_page_ext(ext);
    }
    opts.exts.iter().any(|e| wildcard_match(e.trim_start_matches('.'), ext))
}

pub(super) fn allowed(opts: &GrabOptions, url: &str) -> bool {
    if opts.exclude.iter().any(|p| !p.is_empty() && wildcard_match(p, url)) {
        return false;
    }
    opts.include.iter().all(|p| p.is_empty()) || opts.include.iter().any(|p| !p.is_empty() && wildcard_match(p, url))
}

/// Worth asking the server about: an address that names no file but looks
/// like a download ("download.php?id=5", "/get/123", "?dl=1").
pub fn looks_like_download(url: &str, text: &str) -> bool {
    let l = url.to_ascii_lowercase();
    let t = text.to_lowercase();
    let words = ["download", "/dl/", "dl=", "/get/", "get.php", "file", "attachment", "/d/", "mirror", "fetch"];
    words.iter().any(|w| l.contains(w)) || ["download", "تحميل", "تنزيل"].iter().any(|w| t.contains(w))
}


/// File types of the grabber's ready-made filters.
pub fn template_exts(template: i32) -> Vec<String> {
    let list = match template {
        // all images
        1 => "jpg jpeg png gif webp bmp svg tif tiff ico heic",
        // all videos
        2 => "mp4 mkv avi mov wmv flv webm m4v ts 3gp mpg mpeg",
        // all music
        3 => "mp3 wav flac aac ogg m4a wma opus",
        // archives + programs
        4 => "zip rar 7z tar gz tgz bz2 xz iso exe msi apk dmg",
        // documents
        5 => "pdf doc docx xls xlsx ppt pptx txt epub",
        // every file
        _ => "",
    };
    list.split_whitespace().map(String::from).collect()
}
