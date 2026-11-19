use super::{add_part, chosen, is_web, part_name, show_length, PREVIEW_MAX, STATE};
use crate::i18n::{t, tf};
use crate::*;
use std::sync::Arc;
use swiftget_engine::ytdlp::{self, YtOption};
use swiftget_engine::{clock, Clip, ClipSource, SubtitleTask};

pub(super) fn fetch_video(d: &ClipDialog) {
    let url = d.get_url().trim().to_string();
    if !is_web(&url) {
        d.set_message(t("Paste the address of the video (https://…).").into());
        return;
    }
    d.set_message(SharedString::new());
    d.set_busy(true);
    d.set_ready(false);
    d.set_video_title(SharedString::new());
    d.set_status(t("Looking at the video… (a few seconds)").into());
    let w2 = d.as_weak();
    std::thread::spawn(move || {
        let result = ytdlp::resolve_for(&url, ytdlp::remembered_cookies(&url).as_deref(), None);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = w2.upgrade() else { return };
            d.set_busy(false);
            match result {
                Ok(info) => {
                    let options: Vec<YtOption> = info.options.into_iter().filter(|o| is_web(&o.video_url)).collect();
                    if options.is_empty() {
                        d.set_status(t("No video was found at this address.").into());
                        return;
                    }
                    let labels: Vec<SharedString> = options
                        .iter()
                        .map(|o| if o.sub.is_empty() { o.label.clone().into() } else { format!("{} — {}", o.label, o.sub).into() })
                        .collect();
                    let dur = options.iter().map(|o| o.duration).max().unwrap_or(0) as f32;
                    d.set_qualities(ModelRc::new(VecModel::from(labels)));
                    d.set_quality(0);
                    let sub_labels: Vec<SharedString> = info
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
                    d.set_sub_langs(ModelRc::new(VecModel::from(sub_labels)));
                    d.set_sub_lang(0);
                    d.set_sub_mode(0);
                    d.set_duration(dur);
                    d.set_from(0.0);
                    let to = if dur > 0.0 { dur.min(30.0) } else { 0.0 };
                    d.set_to(to);
                    d.set_from_text(clock(0.0, ':').into());
                    d.set_to_text(if to > 0.0 { clock(to as f64, ':').into() } else { SharedString::new() });
                    show_length(&d);
                    d.set_video_title(info.title.clone().into());
                    d.set_status(if dur > 0.0 {
                        tf("Length of the video: {}", &[&clock(dur as f64, ':')]).into()
                    } else {
                        t("The length of this video is not known: type the times.").into()
                    });
                    d.set_ready(true);
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        s.page = url.clone();
                        s.title = info.title;
                        s.options = options;
                        s.subtitles = info.subtitles;
                    });
                }
                Err(e) => d.set_status(tf("Could not get the video: {}", &[&crate::err_text(&format!("{e:#}"))]).into()),
            }
        });
    });
}

pub(super) fn preview_part(d: &ClipDialog, mgr: &Arc<Manager>) {
    let c = match chosen(d) {
        Ok(c) => c,
        Err(m) => return d.set_message(m.into()),
    };
    let picked = d.get_quality().max(0) as usize;
    let Some((opt, sound_only)) = STATE.with(|s| {
        let s = s.borrow();
        let mine = s.options.get(picked)?.clone();
        if mine.key.starts_with("audio|") {
            return Some((mine, true));
        }
        // the smallest picture is enough to see the part (and quick)
        let small = s
            .options
            .iter()
            .filter(|o| !o.key.starts_with("audio|"))
            .min_by_key(|o| {
                let h: u32 = o.quality.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(9999);
                if h < 200 { 10_000 + h } else { h }
            })
            .cloned()
            .unwrap_or(mine);
        Some((small, false))
    }) else {
        return;
    };
    let c = Clip { end: c.end.min(c.start + PREVIEW_MAX), ..c };
    let mut headers = RequestHeaders::default();
    ytdlp::apply_headers(&mut headers, &opt.headers);
    let src = ClipSource { url: opt.video_url.clone(), audio_url: opt.audio_url.clone(), headers, hls: false };
    d.set_previewing(true);
    d.set_message(SharedString::new());
    let w2 = d.as_weak();
    let token = mgr.preview_part(src, c, sound_only, move |r| {
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = w2.upgrade() else { return };
            d.set_previewing(false);
            match r {
                Ok(file) => {
                    if open::that_detached(&file).is_err() {
                        d.set_message(t("No program on this computer plays this kind of file.").into());
                    }
                }
                Err(e) if e.contains("cancelled") || e.contains("stopped") => {}
                Err(e) => d.set_message(tf("Could not prepare the preview: {}", &[&crate::err_text(&e)]).into()),
            }
        });
    });
    STATE.with(|s| {
        if let Some(old) = s.borrow_mut().preview.replace(token) {
            old.cancel();
        }
    });
}

pub(super) fn queue_part(d: &ClipDialog, ui: &Ui) {
    let c = match chosen(d) {
        Ok(c) => c,
        Err(m) => return d.set_message(m.into()),
    };
    let picked = d.get_quality().max(0) as usize;
    let sub_mode = d.get_sub_mode();
    let sub_idx = d.get_sub_lang().max(0) as usize;
    let Some((opt, page, title, sub)) = STATE.with(|s| {
        let s = s.borrow();
        let opt = s.options.get(picked)?.clone();
        let sub = (sub_mode > 0).then(|| s.subtitles.get(sub_idx).cloned()).flatten();
        Some((opt, s.page.clone(), s.title.clone(), sub))
    }) else {
        return;
    };
    let subtitle = sub.map(|s| SubtitleTask {
        lang: s.lang,
        url: Some(s.url),
        burn: sub_mode == 3,
        extra_headers: s.headers,
        format: Some(if sub_mode == 2 { "vtt" } else { "srt" }.into()),
        ..Default::default()
    });
    let mut headers = RequestHeaders::default();
    ytdlp::apply_headers(&mut headers, &opt.headers);
    let req = NewDownload {
        url: opt.video_url.clone(),
        file_name: Some(part_name(&title, &opt.ext, &opt.key)),
        headers,
        audio_url: opt.audio_url.clone(),
        source_page: Some(page),
        source_key: Some(opt.key.clone()),
        clip: Some(c),
        subtitle,
        start: true,
        ..Default::default()
    };
    add_part(ui, req);
    d.set_message(SharedString::new());
    d.set_status(tf("The part {} was added to the list.", &[&c.label()]).into());
}
