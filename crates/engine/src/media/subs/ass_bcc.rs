use super::Cue;

fn parse_ass_ts(s: &str) -> Option<u64> {
    let mut parts = s.trim().split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let rest = parts.next()?;
    let (sec_s, frac_s) = rest.split_once('.').unwrap_or((rest, "0"));
    let sec: u64 = sec_s.parse().ok()?;
    let mut frac = frac_s.chars().take(3).collect::<String>();
    while frac.len() < 3 {
        frac.push('0');
    }
    let ms: u64 = frac.parse().ok()?;
    Some(((h * 3600 + m * 60 + sec) * 1000).saturating_add(ms))
}

fn strip_ass_tags(raw: &str) -> String {
    let replaced = raw.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ");
    let mut out = String::with_capacity(replaced.len());
    let mut depth = 0u32;
    for ch in replaced.chars() {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            out.push(ch);
        }
    }
    out
}

/// Parse ASS / SSA `[Events]` `Dialogue:` lines into normalized cues.
pub(super) fn parse_ass(raw: &str) -> Vec<Cue> {
    let mut in_events = false;
    let (mut start_col, mut end_col, mut text_col, mut total_cols) = (1usize, 2usize, 9usize, 10usize);
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_events = t.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(fmt) = t.strip_prefix("Format:").or_else(|| t.strip_prefix("format:")) {
            let cols: Vec<&str> = fmt.split(',').map(str::trim).collect();
            if cols.len() >= 3 {
                total_cols = cols.len();
                for (idx, c) in cols.iter().enumerate() {
                    if c.eq_ignore_ascii_case("Start") {
                        start_col = idx;
                    } else if c.eq_ignore_ascii_case("End") {
                        end_col = idx;
                    } else if c.eq_ignore_ascii_case("Text") {
                        text_col = idx;
                    }
                }
            }
            continue;
        }
        let Some(body) = t.strip_prefix("Dialogue:").or_else(|| t.strip_prefix("dialogue:")) else {
            continue;
        };
        let fields: Vec<&str> = body.splitn(total_cols.max(text_col + 1), ',').collect();
        let (Some(s_raw), Some(e_raw), Some(txt_raw)) = (fields.get(start_col), fields.get(end_col), fields.get(text_col)) else {
            continue;
        };
        let (Some(start_ms), Some(end_ms)) = (parse_ass_ts(s_raw), parse_ass_ts(e_raw)) else {
            continue;
        };
        let text = strip_ass_tags(txt_raw).trim().to_string();
        if !text.is_empty() {
            out.push(Cue { start_ms, end_ms, text });
        }
    }
    out
}

/// Parse Bilibili `.bcc` JSON captions (`{ "body": [{ "from", "to", "content" }] }`).
pub(super) fn parse_bcc(raw: &str) -> Vec<Cue> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(body) = v["body"].as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(body.len());
    for item in body {
        let Some(from) = item["from"].as_f64() else { continue };
        let to = item["to"].as_f64().unwrap_or(from + 2.0);
        let Some(content) = item["content"].as_str() else { continue };
        let text = content.trim().to_string();
        if !text.is_empty() {
            out.push(Cue {
                start_ms: (from.max(0.0) * 1000.0).round() as u64,
                end_ms: (to.max(from + 0.2) * 1000.0).round() as u64,
                text,
            });
        }
    }
    out
}
