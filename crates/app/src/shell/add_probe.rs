use super::add_open::show_name_in_dialog;
use super::state::Ctx;
use crate::i18n::{t, tf};
use swiftget_engine::sync::LockOk;
use swiftget_engine::{fmt_size, DownloadKind};

/// Ask the server about the file in the background, then show the result.
pub(super) fn start_probe(ctx: &Ctx, url: String) {
    let Some(d) = ctx.add.upgrade() else { return };
    let (seq, mut headers, audio_url) = {
        let mut st = ctx.add_state.lock_ok();
        st.probe_seq += 1;
        st.probe = None;
        let audio = st.extra.audio_url.clone().filter(|_| st.origin_url == url);
        (st.probe_seq, st.headers.clone(), audio)
    };
    if d.get_use_login() && !d.get_user().trim().is_empty() {
        headers.auth = Some((d.get_user().trim().to_string(), d.get_password().to_string()));
    }
    d.set_probing(true);
    d.set_info_error(false);
    let ctx2 = ctx.clone();
    ctx.rt.spawn(async move {
        // picture and sound come apart: both sizes, so the window shows the
        // whole size (as the browser add-in does)
        let (result, audio_size) = match &audio_url {
            Some(a) => {
                let (v, s) = tokio::join!(ctx2.mgr.probe(&url, &headers), ctx2.mgr.probe(a, &headers));
                (v, s.ok().and_then(|p| p.total_size))
            }
            None => (ctx2.mgr.probe(&url, &headers).await, None),
        };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = ctx2.add.upgrade() else { return };
            let mut st = ctx2.add_state.lock_ok();
            if st.probe_seq != seq {
                return; // the window was reused for something else meanwhile
            }
            d.set_probing(false);
            match result {
                Ok(p) => {
                    if d.get_save_as().as_str() == st.auto_path && !st.auto_path.is_empty() {
                        drop(st);
                        let path = show_name_in_dialog(&ctx2, &d, &p.file_name, &url);
                        st = ctx2.add_state.lock_ok();
                        st.auto_path = path;
                    }
                    let has_audio = st.extra.audio_url.is_some() && st.origin_url == url;
                    let size = p.total_size.map(fmt_size).unwrap_or_else(|| t("Unknown size"));
                    let size_text = match (has_audio, p.total_size, audio_size) {
                        (true, Some(v), Some(a)) => {
                            format!("{}\n{}\n{}", fmt_size(v + a), tf("picture {}", &[&fmt_size(v)]), tf("sound {}", &[&fmt_size(a)]))
                        }
                        (true, _, _) => format!("{size}\n{}", t("+ audio")),
                        _ => size,
                    };
                    d.set_size_text(size_text.into());
                    let kind = match (p.kind, p.resumable) {
                        _ if has_audio => "Video + audio, joined after download",
                        (DownloadKind::Hls, _) => "Video stream (HLS)",
                        (_, true) => "Resume supported",
                        (_, false) => "No resume support",
                    };
                    d.set_can_use_browser(false);
                    d.set_info(format!("{}  ·  {}", t(kind), p.content_type.clone().unwrap_or_default()).into());
                    d.set_info_error(false);
                    if p.is_web_page() {
                        d.set_warning(
                            t("This address opens a web page, not a file. The link may have expired or need a login.")
                                .into(),
                        );
                    }
                    st.probe_url = url;
                    st.probe = Some(p);
                }
                Err(e) => {
                    let text = format!("{e:#}");
                    // refused (often a site that only serves its own browser
                    // session): the browser can still take it
                    d.set_can_use_browser(["HTTP 403", "HTTP 406", "HTTP 429"].iter().any(|c| text.contains(c)));
                    d.set_info(text.into());
                    d.set_info_error(true);
                }
            }
        });
    });
}
