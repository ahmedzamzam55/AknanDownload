use super::add_open::show_name_in_dialog;
use super::state::{AddMediaState, Ctx};
use crate::i18n::t;
use crate::AddDialog;
use slint::{ModelRc, SharedString, VecModel};
use swiftget_engine::sync::LockOk;
use swiftget_engine::ytdlp::{self, YtInfo};
use swiftget_engine::{filename, fmt_size, NewDownload, SubtitleTask};

const VIDEO_HOSTS: &[&str] = &[
    "youtube.com", "youtu.be", "youtubekids.com", "vimeo.com", "tiktok.com",
    "instagram.com", "facebook.com", "fb.watch", "dailymotion.com", "dai.ly",
    "twitch.tv", "reddit.com", "x.com", "twitter.com",
];

pub(crate) fn is_video_page_candidate(url: &str, web_page: bool) -> bool {
    if web_page {
        return true;
    }
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| VIDEO_HOSTS.iter().any(|d| h == *d || h.ends_with(&format!(".{d}"))))
}

pub(super) fn reset_media_panel(ctx: &Ctx, d: &AddDialog) {
    ctx.add_state.lock_ok().media = None;
    d.set_media_open(false);
    d.set_media_resolving(false);
    d.set_media_qualities(ModelRc::new(VecModel::<SharedString>::default()));
    d.set_media_sub_langs(ModelRc::new(VecModel::<SharedString>::default()));
    d.set_media_quality_index(0);
    d.set_media_sub_lang_index(0);
    d.set_media_sub_mode_index(0);
}

pub(super) fn trigger_resolve_if_needed(ctx: &Ctx, seq: u64, url: &str, web_page: bool) {
    let skip = {
        let st = ctx.add_state.lock_ok();
        st.origin_url == url && st.extra.source_key.is_some()
    };
    if skip || !is_video_page_candidate(url, web_page) {
        return;
    }
    let Some(d) = ctx.add.upgrade() else { return };
    d.set_media_resolving(true);
    let ctx2 = ctx.clone();
    let url2 = url.to_string();
    std::thread::spawn(move || {
        let result = ytdlp::resolve_for(&url2, ytdlp::remembered_cookies(&url2).as_deref(), None);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = ctx2.add.upgrade() else { return };
            let mut st = ctx2.add_state.lock_ok();
            if st.probe_seq != seq {
                return;
            }
            d.set_media_resolving(false);
            let Ok(info) = result else { return };
            if info.options.is_empty() && info.subtitles.is_empty() {
                return;
            }
            let q_labels: Vec<SharedString> = info
                .options
                .iter()
                .map(|o| if o.sub.is_empty() { o.label.clone().into() } else { format!("{} — {}", o.label, o.sub).into() })
                .collect();
            let s_labels: Vec<SharedString> = info
                .subtitles
                .iter()
                .map(|s| {
                    if s.auto {
                        format!("{} ({}) · {}", s.name, s.lang, t("auto")).into()
                    } else {
                        format!("{} ({})", s.name, s.lang).into()
                    }
                })
                .collect();
            d.set_media_qualities(ModelRc::new(VecModel::from(q_labels)));
            d.set_media_sub_langs(ModelRc::new(VecModel::from(s_labels)));
            d.set_media_quality_index(0);
            d.set_media_sub_lang_index(0);
            d.set_media_sub_mode_index(0);
            d.set_media_open(true);
            d.set_warning(SharedString::new());
            d.set_info(t("Choose quality and subtitles below").into());
            d.set_info_error(false);
            st.media = Some(AddMediaState { source_url: url2.clone(), info });
            drop(st);
            apply_selection_to_ui(&ctx2, &d);
        });
    });
}

fn target_name_for(info: &YtInfo, q_idx: usize, mode: i32, s_idx: usize) -> String {
    let base = filename::sanitize(info.title.trim());
    let stem = if base.is_empty() { "video".to_string() } else { base };
    if mode >= 4 {
        let lang = info.subtitles.get(s_idx).map(|s| filename::sanitize(&s.lang)).unwrap_or_else(|| "sub".into());
        let ext = if mode == 5 { "vtt" } else { "srt" };
        return format!("{stem}.{lang}.{ext}");
    }
    if let Some(opt) = info.options.get(q_idx) {
        if opt.key == ytdlp::MP3_KEY {
            return format!("{stem}.mp3");
        }
        return format!("{stem} [{}].{}", opt.quality, opt.ext);
    }
    format!("{stem}.mp4")
}

pub(super) fn apply_selection_to_ui(ctx: &Ctx, d: &AddDialog) {
    let st = ctx.add_state.lock_ok();
    let Some(m) = &st.media else { return };
    let q_idx = d.get_media_quality_index().max(0) as usize;
    let mode = d.get_media_sub_mode_index();
    let s_idx = d.get_media_sub_lang_index().max(0) as usize;
    let name = target_name_for(&m.info, q_idx, mode, s_idx);
    let url = m.source_url.clone();
    let size_text = if mode >= 4 {
        t("Subtitle file")
    } else {
        m.info.options.get(q_idx).map(|o| if o.size > 0 { fmt_size(o.size) } else { t("Unknown size") }).unwrap_or_default()
    };
    drop(st);
    let (folder, final_name) = show_name_in_dialog(ctx, d, &name, &url);
    d.set_size_text(size_text.into());
    d.set_conflict_open(false);
    let mut st = ctx.add_state.lock_ok();
    st.auto_folder = folder;
    st.auto_name = final_name;
}

/// Apply the user's selected resolved quality / subtitle / standalone subtitle
/// inside `AddDialog` right before queuing or starting the download.
pub(super) fn apply_media_choice(ctx: &Ctx, d: &AddDialog, req: &mut NewDownload, typed_name: &str) -> bool {
    let st = ctx.add_state.lock_ok();
    let Some(m) = &st.media else { return false };
    if m.source_url != req.url {
        return false;
    }
    let q_idx = d.get_media_quality_index().max(0) as usize;
    let mode = d.get_media_sub_mode_index();
    let s_idx = d.get_media_sub_lang_index().max(0) as usize;
    if mode >= 4 {
        let Some(sub) = m.info.subtitles.get(s_idx) else { return false };
        let fmt = if mode == 5 { "vtt" } else { "srt" };
        let auto_name = target_name_for(&m.info, q_idx, mode, s_idx);
        let name = if typed_name.trim().is_empty() { auto_name } else { filename::sanitize(typed_name) };
        req.file_name = Some(name);
        req.url = sub.url.clone();
        req.audio_url = None;
        req.source_page = Some(m.source_url.clone());
        req.source_key = None;
        req.probe = None;
        req.sub_convert = Some(fmt.into());
        req.subtitle = None;
        ytdlp::apply_headers(&mut req.headers, &sub.headers);
        return true;
    }
    let Some(opt) = m.info.options.get(q_idx) else { return false };
    req.url = opt.video_url.clone();
    req.audio_url = opt.audio_url.clone();
    req.source_page = Some(m.source_url.clone());
    req.source_key = Some(opt.key.clone());
    req.probe = None;
    ytdlp::apply_headers(&mut req.headers, &opt.headers);
    if typed_name.trim().is_empty() {
        req.file_name = Some(target_name_for(&m.info, q_idx, mode, s_idx));
    }
    if (1..=3).contains(&mode) {
        if let Some(sub) = m.info.subtitles.get(s_idx) {
            req.subtitle = Some(SubtitleTask {
                lang: sub.lang.clone(),
                url: Some(sub.url.clone()),
                burn: mode == 3,
                extra_headers: sub.headers.clone(),
                format: Some(if mode == 2 { "vtt" } else { "srt" }.into()),
                ..Default::default()
            });
        }
    }
    true
}
