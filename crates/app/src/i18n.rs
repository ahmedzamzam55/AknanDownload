//! The texts made in Rust (statuses, messages) in the language of the
//! program: lang/rs/<code>.json (English text -> translation). The window
//! texts themselves are translated by Slint (lang/<code>/LC_MESSAGES/*.po).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The languages of the program: code and own name, in the order of the
/// Language list in Options.
pub const LANGS: [(&str, &str); 8] = [
    ("en", "English"),
    ("ar", "العربية"),
    ("fr", "Français"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("pt", "Português"),
    ("tr", "Türkçe"),
    ("ru", "Русский"),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// A language code from LANGS, or "" = the language of Windows.
pub fn effective(setting: &str) -> &'static str {
    if let Some((code, _)) = LANGS.iter().find(|(c, _)| *c == setting) {
        return code;
    }
    let sys = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
    LANGS.iter().map(|(c, _)| *c).find(|c| sys.starts_with(c)).unwrap_or("en")
}

/// Place of a language in LANGS (0 = English for anything unknown).
pub fn index_of(code: &str) -> usize {
    LANGS.iter().position(|(c, _)| *c == code).unwrap_or(0)
}

/// Switch every window and every Rust text to this language.
pub fn apply(lang: &str) {
    CURRENT.store(index_of(lang), Ordering::Relaxed);
    let _ = slint::select_bundled_translation(LANGS[index_of(lang)].0);
}

pub fn current() -> &'static str {
    LANGS[CURRENT.load(Ordering::Relaxed) % LANGS.len()].0
}

/// The windows read from right to left (Arabic).
pub fn is_rtl() -> bool {
    current() == "ar"
}

#[allow(dead_code)]
pub fn is_arabic() -> bool {
    current() == "ar"
}

fn table(code: &str) -> Option<&'static HashMap<String, String>> {
    static AR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static ES: OnceLock<HashMap<String, String>> = OnceLock::new();
    static DE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static PT: OnceLock<HashMap<String, String>> = OnceLock::new();
    static TR: OnceLock<HashMap<String, String>> = OnceLock::new();
    static RU: OnceLock<HashMap<String, String>> = OnceLock::new();
    let (cell, src) = match code {
        "ar" => (&AR, include_str!("../lang/rs/ar.json")),
        "fr" => (&FR, include_str!("../lang/rs/fr.json")),
        "es" => (&ES, include_str!("../lang/rs/es.json")),
        "de" => (&DE, include_str!("../lang/rs/de.json")),
        "pt" => (&PT, include_str!("../lang/rs/pt.json")),
        "tr" => (&TR, include_str!("../lang/rs/tr.json")),
        "ru" => (&RU, include_str!("../lang/rs/ru.json")),
        _ => return None,
    };
    Some(cell.get_or_init(|| serde_json::from_str(src).unwrap_or_default()))
}

/// The text in the current language.
pub fn t(en: &str) -> String {
    if current() != "en" {
        if let Some(v) = table(current()).and_then(|m| m.get(en)) {
            return v.clone();
        }
    }
    en.to_string()
}

/// A text with "{}" places, filled in order.
pub fn tf(en: &str, args: &[&dyn std::fmt::Display]) -> String {
    let template = t(en);
    let mut out = String::new();
    let mut rest = template.as_str();
    let mut i = 0;
    while let Some(p) = rest.find("{}") {
        out.push_str(&rest[..p]);
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        i += 1;
        rest = &rest[p + 2..];
    }
    out.push_str(rest);
    out
}

/// A name written right to left (Arabic, Hebrew, Persian…): its first
/// letter decides, as in a text editor.
pub fn rtl_text(s: &str) -> bool {
    for c in s.chars() {
        let u = c as u32;
        if matches!(u, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF) {
            return true;
        }
        if c.is_alphabetic() {
            return false;
        }
    }
    false
}

/// The name as it is shown in a list: a right-to-left one starts with the
/// invisible right-to-left mark, so numbers and Latin words in it keep
/// their place.
pub fn shown_name(s: &str) -> String {
    if rtl_text(s) {
        format!("\u{200F}{s}")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_places() {
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "Tip 1 of 14");
        CURRENT.store(index_of("ar"), Ordering::Relaxed);
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "نصيحة 1 من 14");
        assert_eq!(t("not in the list"), "not in the list");
        CURRENT.store(index_of("fr"), Ordering::Relaxed);
        assert_eq!(tf("Tip {} of {}", &[&1, &14]), "Astuce 1 sur 14");
        CURRENT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn every_language_has_every_text() {
        // the tables are read from JSON: a broken file would silently fall back to English
        for (code, _) in LANGS.iter().skip(1) {
            let m = table(code).expect("table");
            assert!(m.len() > 150, "{code}: only {} texts", m.len());
            for (en, tr) in m {
                assert_eq!(en.matches("{}").count(), tr.matches("{}").count(), "{code}: {en}");
            }
        }
        assert_eq!(index_of("xx"), 0);
        assert_eq!(effective("es"), "es");
    }

    #[test]
    fn whats_new_and_shortcuts_in_every_language() {
        let texts: Vec<&str> = crate::extra::WHATS_NEW
            .iter()
            .copied()
            .chain([crate::SHORTCUTS, "Checking the file", "Burning subtitles into the video"])
            .collect();
        for en in texts {
            for (code, _) in LANGS.iter().skip(1) {
                assert!(table(code).expect("table").contains_key(en), "{code}: {en}");
            }
        }
    }
}

#[cfg(test)]
mod rtl_tests {
    #[test]
    fn first_letter_decides() {
        assert!(super::rtl_text("07 - طريقة عمل الأرز"));
        assert!(super::rtl_text("שלום world"));
        assert!(!super::rtl_text("How to — طريقة"));
        assert!(!super::rtl_text("123"));
        assert_eq!(super::shown_name("abc"), "abc");
        assert_eq!(super::shown_name("أ"), "\u{200F}أ");
    }
}
