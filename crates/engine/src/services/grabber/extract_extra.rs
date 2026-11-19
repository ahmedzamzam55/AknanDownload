use regex::Regex;
use std::sync::OnceLock;

fn base_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?is)<base\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap())
}

fn srcset_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\b(?:srcset|data-srcset)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap())
}

fn extra_attr_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(?:data-url|data-download|data-file|data-link|data-original|data-lazy-src|data-bg)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))|<(?:object|embed)\b[^>]*?\b(?:data|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))|\b(?:href|src)\s*=\s*([^\s"'<>`]+)"#,
        )
        .unwrap()
    })
}

fn css_url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\burl\(\s*(?:"([^"]*)"|'([^']*)'|([^)\s"']+))\s*\)"#).unwrap())
}

fn json_rel_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"https?:(?:\\/|\\u002[fF]){2}[^\s"'<>\\]+|\\u002[fF][^\s"'<>)]+"#).unwrap())
}

fn script_style_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<(?:script|style)\b.*?</(?:script|style)\s*>|<[^>]*>").unwrap())
}

pub(super) fn base_href(html: &str, page_url: &url::Url) -> url::Url {
    if let Some(c) = base_re().captures(html) {
        let raw = c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str().trim()).unwrap_or("");
        if let Ok(u) = page_url.join(raw) {
            if matches!(u.scheme(), "http" | "https") {
                return u;
            }
        }
    }
    page_url.clone()
}

pub(super) fn decode_html_entities(s: &str) -> String {
    let basic = s
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let mut out = String::with_capacity(basic.len());
    let mut rest = basic.as_str();
    while let Some(amp) = rest.find("&#") {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 2..];
        if let Some(semi) = after.find(';').filter(|&i| i > 0 && i <= 8) {
            let code = &after[..semi];
            let parsed = if let Some(hex) = code.strip_prefix('x').or_else(|| code.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                code.parse::<u32>().ok()
            };
            if let Some(ch) = parsed.and_then(char::from_u32) {
                out.push(ch);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push_str("&#");
        rest = after;
    }
    out.push_str(rest);
    out
}

pub(super) fn decode_json_url(s: &str) -> String {
    s.replace("\\/", "/")
        .replace("\\u0026", "&")
        .replace("\\u002f", "/")
        .replace("\\u002F", "/")
        .replace("\\u003d", "=")
        .replace("\\u003D", "=")
        .replace("\\u003f", "?")
        .replace("\\u003F", "?")
        .replace("\\u003a", ":")
        .replace("\\u003A", ":")
}

pub(super) fn push_extra_links(html: &str, mut push: impl FnMut(&str, String)) {
    for c in srcset_re().captures_iter(html) {
        let val = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
        for item in val.split(',') {
            if let Some(url) = item.split_whitespace().next() {
                push(url, String::new());
            }
        }
    }
    for c in extra_attr_re().captures_iter(html) {
        for gi in 1..=7 {
            if let Some(m) = c.get(gi) {
                push(m.as_str(), String::new());
                break;
            }
        }
    }
    for c in css_url_re().captures_iter(html) {
        if let Some(m) = c.get(1).or(c.get(2)).or(c.get(3)) {
            push(m.as_str(), String::new());
        }
    }
    for m in json_rel_re().find_iter(html) {
        let raw = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?', '\\']);
        push(&decode_json_url(raw), String::new());
    }
}

pub fn looks_dynamic_spa(html: &str) -> bool {
    let low = html.to_ascii_lowercase();
    let has_shell = [
        "id=\"root\"",
        "id='root'",
        "id=\"app\"",
        "id='app'",
        "id=\"__next\"",
        "__next_data__",
        "__nuxt__",
        "ng-version=",
    ]
    .iter()
    .any(|m| low.contains(m));
    let noscript_js = low.contains("<noscript>") && (low.contains("javascript") || low.contains("enable js"));
    let scripts = low.matches("<script").count();
    let visible_chars = script_style_re()
        .replace_all(html, " ")
        .split_whitespace()
        .map(str::len)
        .sum::<usize>();
    noscript_js || (has_shell && scripts >= 2 && visible_chars < 200)
}
