use crate::PlaylistDialog;
use swiftget_engine::{Settings, SubtitleTask};

/// Populate the playlist dialog's subtitle mode & language from global settings.
pub(super) fn apply_defaults(d: &PlaylistDialog, s: &Settings) {
    d.set_sub_mode(if s.subtitles { 1 } else { 0 });
    let langs = if s.subtitle_langs.trim().is_empty() {
        "ar en"
    } else {
        s.subtitle_langs.trim()
    };
    d.set_sub_lang(langs.into());
}

/// Convert the playlist subtitle dropdown (`0..=5`) + language string into
/// `(pick_quality, target_ext, subtitle_task, sub_convert)`.
pub(super) fn request_fields(
    sub_mode: i32,
    sub_lang: &str,
    quality: &str,
    video_ext: &str,
) -> (String, String, Option<SubtitleTask>, Option<String>) {
    let langs = if sub_lang.trim().is_empty() { "ar en" } else { sub_lang.trim() }.to_string();
    let audio_only = matches!(quality, "audio" | "mp3");
    match sub_mode {
        1 if !audio_only => (
            quality.to_string(),
            video_ext.to_string(),
            Some(SubtitleTask { lang: langs, format: Some("srt".into()), ..Default::default() }),
            None,
        ),
        2 if !audio_only => (
            quality.to_string(),
            video_ext.to_string(),
            Some(SubtitleTask { lang: langs, format: Some("vtt".into()), ..Default::default() }),
            None,
        ),
        3 if !audio_only => (
            quality.to_string(),
            video_ext.to_string(),
            Some(SubtitleTask { lang: langs, burn: true, format: Some("srt".into()), ..Default::default() }),
            None,
        ),
        4 => (
            "subs".to_string(),
            "srt".to_string(),
            Some(SubtitleTask { lang: langs, ..Default::default() }),
            Some("srt".into()),
        ),
        5 => (
            "subs".to_string(),
            "vtt".to_string(),
            Some(SubtitleTask { lang: langs, ..Default::default() }),
            Some("vtt".into()),
        ),
        _ => (quality.to_string(), video_ext.to_string(), None, None),
    }
}
