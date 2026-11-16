use super::add::cancel_early;
use super::add_probe::start_probe;
use super::complete::show_complete;
use super::details::open_progress;
use super::format::{kind_for, queue_names};
use super::rows::refresh;
use super::state::{Ctx, PendingAdd, PendingOpen, Ui, VideoExtra};
use crate::i18n::t;
use crate::{torrent_ui, AddDialog};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use std::path::{Path, PathBuf};
use std::time::Duration;
use swiftget_engine::sync::LockOk;
use swiftget_engine::{filename, NewDownload, RequestHeaders, Status};

/// Fill "Download File Info" with a name: category, Save As, icon.
pub(super) fn show_name_in_dialog(ctx: &Ctx, d: &AddDialog, name: &str, url: &str) -> String {
    let s = ctx.mgr.settings();
    let cat = s.category_of(name, url);
    let path = s.dir_for(&cat).join(name).display().to_string();
    d.set_save_as(path.clone().into());
    let cats = ctx.add_state.lock_ok().categories.clone();
    d.set_category_index(cats.iter().position(|c| *c == cat).unwrap_or(0) as i32);
    d.set_category_name(t(&cat).into());
    // a category with a remembered folder: the box stays ticked (shows it is
    // remembered; untick it to forget)
    d.set_remember_path(s.category_dirs.contains_key(&cat));
    d.set_kind(kind_for(name).into());
    d.set_ext(filename::extension(name).to_uppercase().into());
    path
}

pub(crate) fn open_add_dialog(ctx: &Ctx, url: String, headers: RequestHeaders, name_hint: Option<String>, extra: VideoExtra) {
    // a magnet link: the torrent window
    if swiftget_engine::torrent::is_magnet(&url) {
        torrent_ui::open_source(&url);
        return;
    }
    let Some(d) = ctx.add.upgrade() else { return };
    let s = ctx.mgr.settings();
    if d.window().is_visible() {
        let mut st = ctx.add_state.lock_ok();
        // the same link again (a page can send it twice): nothing new to show;
        // another link waits until this one is started or cancelled
        if !url.is_empty() && st.origin_url != url && !st.waiting.iter().any(|p| p.url == url) {
            st.waiting.push_back(PendingOpen { url, headers, name_hint, extra });
        }
        drop(st);
        to_front!(d);
        d.invoke_focus_input();
        return;
    }
    // nothing may be left over from an earlier window
    cancel_early(ctx);
    // General first, like the classic list
    let mut cats = vec!["General".to_string()];
    cats.extend(s.categories.iter().map(|c| c.name.clone()));
    let names: Vec<SharedString> = cats.iter().map(|c| t(c).into()).collect();
    d.set_category_names(ModelRc::new(VecModel::from(names)));
    ctx.add_state.lock_ok().categories = cats;
    let name = name_hint
        .clone()
        .filter(|n| !n.trim().is_empty())
        .map(|n| filename::sanitize(&n))
        .or_else(|| filename::from_url(&url).map(|n| filename::sanitize(&n)))
        .unwrap_or_else(|| "download".into());
    let path = show_name_in_dialog(ctx, &d, &name, &url);
    let login = headers.auth.clone().or_else(|| s.login_for(&url));
    {
        let mut st = ctx.add_state.lock_ok();
        st.headers = headers;
        st.extra = extra;
        st.origin_url = url.clone();
        st.probe = None;
        st.probe_url.clear();
        // a name from the browser is final; one guessed from the URL is not
        st.auto_path = if name_hint.is_some() { String::new() } else { path };
        st.probe_seq += 1;
    }
    d.set_url(url.clone().into());
    d.set_description(SharedString::new());
    d.set_checksum(SharedString::new());
    d.set_size_text(SharedString::new());
    d.set_connections(s.connections() as i32);
    d.set_queue_names(queue_names(&s));
    d.set_queue_index(0);
    d.set_use_login(login.is_some());
    d.set_user(login.as_ref().map(|l| l.0.clone()).unwrap_or_default().into());
    d.set_password(login.map(|l| l.1).unwrap_or_default().into());
    d.set_warning(
        if !url.is_empty() && ctx.mgr.find_url(&url).is_some() {
            t("This address is already in the download list.")
        } else {
            String::new()
        }
        .into(),
    );
    d.set_info(t("Paste a link and press Check").into());
    d.set_can_use_browser(false);
    d.set_info_error(false);
    d.set_probing(false);
    open_window!(d);
    // it already downloads while the window is open
    if !url.is_empty() && s.start_while_info && ctx.mgr.find_url(&url).is_none() {
        let path = PathBuf::from(d.get_save_as().as_str());
        let st = ctx.add_state.lock_ok();
        let req = NewDownload {
            url: url.clone(),
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            save_dir: path.parent().map(Path::to_path_buf),
            headers: st.headers.clone(),
            audio_url: st.extra.audio_url.clone(),
            source_page: st.extra.source_page.clone(),
            source_key: st.extra.source_key.clone(),
            early: true,
            start: true,
            ..Default::default()
        };
        drop(st);
        let id = ctx.mgr.add(req);
        ctx.add_state.lock_ok().early_id = Some(id);
    }
    if !url.is_empty() {
        start_probe(ctx, url);
    }
}

/// A name typed without its extension ("My video") gets the one of the
/// file (".mp4"), so Windows knows how to open it.
pub(crate) fn with_extension_of(name: &str, suggested: &str) -> String {
    let name = name.trim();
    let ext = Path::new(suggested).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let has_ext = Path::new(name)
        .extension()
        .map(|e| !e.is_empty() && e.len() <= 5 && e.to_string_lossy().chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or(false);
    if has_ext || ext.is_empty() || name.is_empty() {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

/// Really add it; open its progress window when it starts.
pub(crate) fn finish_add(ui: &Ui, req: NewDownload) -> u64 {
    let start = req.start;
    let id = ui.ctx.mgr.add(req);
    if start {
        show_progress_for_start(ui, id);
    }
    refresh(ui);
    id
}

/// What to do with a link that is already in the list: 1 add numbered,
/// 2 replace the old one, 3 show / resume the old one.
pub(crate) fn apply_duplicate(ui: &Ui, p: PendingAdd, choice: u32) {
    match choice {
        2 => {
            ui.ctx.mgr.remove(p.existing, true);
            // the old file goes away a moment later; wait for it
            let ui2 = ui.clone();
            Timer::single_shot(Duration::from_millis(1500), move || {
                finish_add(&ui2, p.req);
            });
        }
        3 => match ui.ctx.mgr.get(p.existing) {
            Some(v) if v.status == Status::Completed => show_complete(ui, v.id),
            Some(v) => {
                ui.ctx.mgr.start(v.id);
                open_progress(ui, v.id);
            }
            None => {
                finish_add(ui, p.req);
            }
        },
        _ => {
            finish_add(ui, p.req);
        }
    }
}

/// Progress window for a download that was just started (Options > Downloads).
pub(crate) fn show_progress_for_start(ui: &Ui, id: u64) {
    match ui.ctx.mgr.settings().progress_mode {
        0 => open_progress(ui, id),
        1 => {
            open_progress(ui, id);
            if let Some((d, _)) = ui.progress.borrow().get(&id) {
                d.window().set_minimized(true);
            }
        }
        _ => {}
    }
}
