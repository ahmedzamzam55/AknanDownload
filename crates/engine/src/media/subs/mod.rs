//! Subtitle format parser and converter (WebVTT, SRT, YouTube json3, YouTube srv/TTML XML -> SRT / VTT / TXT).

mod xml;
#[cfg(test)]
mod tests;

use xml::{parse_xml, replace_br, strip_tags, unescape_xml};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cue {
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    pub(super) text: String,
}

/// Convert raw subtitle content from VTT, SRT, YouTube `json3`, or XML (`srv1`..`srv3`/`ttml`)
/// into `"srt"`, `"vtt"`, or plain `"txt"`.
pub fn convert_subtitles(raw: &str, target_fmt: &str) -> String {
    let raw = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = raw.trim();
    let cues = if trimmed.starts_with('{') {
        parse_json3(trimmed)
    } else if trimmed.starts_with('<') {
        parse_xml(trimmed)
    } else {
        parse_vtt_or_srt(trimmed)
    };
    let cues = dedup_cues(cues);
    if cues.is_empty() {
        return trimmed.to_string();
    }
    match target_fmt.to_ascii_lowercase().as_str() {
        "vtt" => format_vtt(&cues),
        "txt" => format_txt(&cues),
        _ => format_srt(&cues),
    }
}

fn dedup_cues(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::with_capacity(cues.len());
    for mut c in cues {
        c.text = c.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        if c.text.is_empty() {
            continue;
        }
        if c.end_ms <= c.start_ms {
            c.end_ms = c.start_ms + 2000;
        }
        if let Some(prev) = out.last_mut() {
            if prev.text == c.text && c.start_ms <= prev.end_ms + 200 {
                prev.end_ms = prev.end_ms.max(c.end_ms);
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn parse_json3(raw: &str) -> Vec<Cue> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { return Vec::new() };
    let Some(events) = v["events"].as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for ev in events {
        let Some(start_ms) = ev["tStartMs"].as_u64() else { continue };
        let dur_ms = ev["dDurationMs"].as_u64().unwrap_or(2000);
        let Some(segs) = ev["segs"].as_array() else { continue };
        let text: String = segs.iter().filter_map(|s| s["utf8"].as_str()).collect();
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(Cue { start_ms, end_ms: start_ms.saturating_add(dur_ms.max(200)), text });
        }
    }
    out
}

fn parse_vtt_or_srt(raw: &str) -> Vec<Cue> {
    let mut out = Vec::new();
    for block in raw.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let head = lines[0].to_ascii_uppercase();
        if head.starts_with("WEBVTT") || head.starts_with("NOTE") || head.starts_with("STYLE") || head.starts_with("REGION") {
            continue;
        }
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let Some((lhs, rhs)) = lines[ti].split_once("-->") else { continue };
        let (Some(start_ms), Some(end_ms)) = (parse_ts(lhs), parse_ts(rhs)) else { continue };
        let body = lines[ti + 1..].join("\n");
        let text = unescape_xml(&strip_tags(&replace_br(&body)));
        if !text.trim().is_empty() {
            out.push(Cue { start_ms, end_ms, text });
        }
    }
    out
}

pub(super) fn parse_ts(s: &str) -> Option<u64> {
    let tok = s.split_whitespace().next()?.trim_end_matches('s');
    if !tok.contains(':') {
        return tok.replace(',', ".").parse::<f64>().ok().map(|v| (v.max(0.0) * 1000.0).round() as u64);
    }
    let parts: Vec<&str> = tok.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let sec_ms = parts.last()?.replace(',', ".").parse::<f64>().ok()?;
    let (h, m) = if parts.len() == 3 {
        (parts[0].parse::<u64>().ok()?, parts[1].parse::<u64>().ok()?)
    } else {
        (0, parts[0].parse::<u64>().ok()?)
    };
    Some(((h * 3600 + m * 60) * 1000).saturating_add((sec_ms.max(0.0) * 1000.0).round() as u64))
}

fn fmt_ms(ms: u64, sep: char) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let r = ms % 1000;
    format!("{h:02}:{m:02}:{s:02}{sep}{r:03}")
}

fn format_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, fmt_ms(c.start_ms, ','), fmt_ms(c.end_ms, ','), c.text));
    }
    out
}

fn format_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", fmt_ms(c.start_ms, '.'), fmt_ms(c.end_ms, '.'), c.text));
    }
    out
}

fn format_txt(cues: &[Cue]) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for c in cues {
        for l in c.text.lines() {
            if lines.last() != Some(&l) {
                lines.push(l);
            }
        }
    }
    lines.join("\n")
}
