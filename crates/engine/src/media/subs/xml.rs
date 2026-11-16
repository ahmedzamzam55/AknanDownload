use super::{parse_ts, Cue};

pub(super) fn parse_xml(raw: &str) -> Vec<Cue> {
    let lower = raw.to_ascii_lowercase();
    let mut out = Vec::new();
    for tag in ["<p", "<text"] {
        let close = if tag == "<p" { "</p>" } else { "</text>" };
        let mut pos = 0;
        while let Some(rel) = lower[pos..].find(tag) {
            let start = pos + rel;
            let next_ch = lower.as_bytes().get(start + tag.len()).copied().unwrap_or(b' ');
            if !matches!(next_ch, b' ' | b'>' | b'\t' | b'\n') {
                pos = start + tag.len();
                continue;
            }
            let Some(gt) = lower[start..].find('>') else { break };
            let attrs = &raw[start..start + gt];
            let body_start = start + gt + 1;
            let Some(end_rel) = lower[body_start..].find(close) else {
                pos = body_start;
                continue;
            };
            let body = &raw[body_start..body_start + end_rel];
            pos = body_start + end_rel + close.len();
            if let Some((start_ms, end_ms)) = xml_times(attrs) {
                let text = unescape_xml(&strip_tags(&replace_br(body)));
                if !text.trim().is_empty() {
                    out.push(Cue { start_ms, end_ms, text });
                }
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

fn attr_val(attrs: &str, name: &str) -> Option<String> {
    let low = attrs.to_ascii_lowercase();
    let pat = format!("{name}=");
    let mut idx = 0;
    while let Some(rel) = low[idx..].find(&pat) {
        let p = idx + rel;
        if p > 0 && !matches!(low.as_bytes()[p - 1], b' ' | b'\t' | b'\n') {
            idx = p + pat.len();
            continue;
        }
        let rest = attrs[p + pat.len()..].trim_start();
        let q = rest.as_bytes().first().copied()?;
        if q == b'"' || q == b'\'' {
            let inner = &rest[1..];
            let end = inner.find(q as char)?;
            return Some(inner[..end].to_string());
        }
        return None;
    }
    None
}

fn xml_times(attrs: &str) -> Option<(u64, u64)> {
    if let Some(t) = attr_val(attrs, "t").and_then(|v| v.parse::<u64>().ok()) {
        let d = attr_val(attrs, "d").and_then(|v| v.parse::<u64>().ok()).unwrap_or(2000);
        return Some((t, t.saturating_add(d)));
    }
    if let Some(s) = attr_val(attrs, "start").and_then(|v| v.parse::<f64>().ok()) {
        let start_ms = (s.max(0.0) * 1000.0).round() as u64;
        let dur_ms = (attr_val(attrs, "dur").and_then(|v| v.parse::<f64>().ok()).unwrap_or(2.0).max(0.2) * 1000.0).round() as u64;
        return Some((start_ms, start_ms.saturating_add(dur_ms)));
    }
    let begin = attr_val(attrs, "begin").and_then(|v| parse_ts(&v))?;
    let end = attr_val(attrs, "end")
        .and_then(|v| parse_ts(&v))
        .or_else(|| attr_val(attrs, "dur").and_then(|v| parse_ts(&v)).map(|d| begin + d))
        .unwrap_or(begin + 2000);
    Some((begin, end))
}

pub(super) fn replace_br(s: &str) -> String {
    s.replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n").replace("<BR>", "\n")
}

pub(super) fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

pub(super) fn unescape_xml(s: &str) -> String {
    let mut out = s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&nbsp;", " ");
    while let Some(a) = out.find("&#") {
        let Some(semi) = out[a..].find(';') else { break };
        let inner = &out[a + 2..a + semi];
        let ch = if let Some(hex) = inner.strip_prefix('x').or_else(|| inner.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else {
            inner.parse::<u32>().ok().and_then(char::from_u32)
        };
        let Some(ch) = ch else { break };
        out.replace_range(a..a + semi + 1, &ch.to_string());
    }
    out
}
