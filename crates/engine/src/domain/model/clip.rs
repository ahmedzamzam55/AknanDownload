use serde::{Deserialize, Serialize};

/// "Download a part of a video": only from `start` to `end` (seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub start: f64,
    pub end: f64,
    /// cut on the very second asked (the start of the picture is made
    /// again: slower); otherwise the quick cut, which starts at the nearest
    /// whole picture before `start` (a second or two earlier, as it is)
    #[serde(default)]
    pub exact: bool,
}

impl Clip {
    /// The longest part (and video) taken: a day.
    pub const MAX: f64 = 24.0 * 3600.0;

    pub fn new(start: f64, end: f64, exact: bool) -> Option<Clip> {
        let ok = start.is_finite() && end.is_finite() && start >= 0.0 && end <= Clip::MAX && end - start >= 0.5;
        ok.then_some(Clip { start, end, exact })
    }
    pub fn len(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
    /// What goes into the file name: "01-20 → 03-45" (Windows takes no ':')
    pub fn label(&self) -> String {
        format!("{} → {}", clock(self.start, '-'), clock(self.end, '-'))
    }
}

/// 75.4 -> "01:15", 3725 -> "1:02:05" (`sep` between the numbers).
pub fn clock(secs: f64, sep: char) -> String {
    let t = secs.max(0.0).floor() as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}{sep}{m:02}{sep}{s:02}")
    } else {
        format!("{m:02}{sep}{s:02}")
    }
}

/// "1:02:05", "02:05", "125", "2:05.5" (Arabic digits too) -> seconds.
pub fn parse_clock(text: &str) -> Option<f64> {
    let t: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            '۰'..='۹' => char::from(b'0' + (c as u32 - '۰' as u32) as u8),
            '٫' | ',' => '.',
            '-' | '：' => ':',
            _ => c,
        })
        .collect();
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t.as_str(), None),
    };
    let nums: Vec<&str> = whole.split(':').collect();
    if whole.is_empty() || nums.len() > 3 || nums.iter().any(|n| n.is_empty() || n.len() > 6 || !n.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let mut total = 0.0;
    for (i, n) in nums.iter().enumerate() {
        let v: f64 = n.parse().ok()?;
        // minutes and seconds after the first number stay under 60
        if i > 0 && v >= 60.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    if let Some(f) = frac {
        if f.is_empty() || f.len() > 3 || !f.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        total += format!("0.{f}").parse::<f64>().ok()?;
    }
    Some(total)
}
