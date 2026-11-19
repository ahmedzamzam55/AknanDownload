use super::format::kind_for;
use super::modal::{confirm_with, message};
use super::rows::refresh;
use super::state::{Ctx, Ui};
use crate::i18n::{t, tf};
use crate::{preview, torrent_ui};
use std::path::Path;
use swiftget_engine::{DownloadKind, DownloadView, Status};

/// "Open with…": Windows' own list of programs for the file. On a thread
/// of its own: the windows never wait for it.
pub(crate) fn open_with(path: &Path) {
    let path = path.to_path_buf();
    #[cfg(windows)]
    std::thread::spawn(move || {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{SHOpenWithDialog, OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OPENASINFO};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let info = OPENASINFO {
            pcszFile: wide.as_ptr(),
            pcszClass: std::ptr::null(),
            oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_EXEC,
        };
        // SAFETY: `wide` is a NUL-terminated path that outlives the call
        let _ = unsafe { SHOpenWithDialog(std::ptr::null_mut(), &info) };
    });
    #[cfg(not(windows))]
    {
        let _ = open::that_detached(path);
    }
}

/// Delete one download, or several with one question.
pub(super) fn delete_downloads(ui: &Ui, ids: Vec<u64>) {
    match ids.len() {
        0 => {}
        1 => delete_download(&ui.ctx, ids[0]),
        n => {
            let ui2 = ui.clone();
            let text = tf("Remove these {} downloads from the download list?", &[&n]);
            ask_remove(&ui.ctx, &t("Delete downloads"), &text, move |delete_files| {
                if delete_files {
                    for id in &ids {
                        ui2.ctx.mgr.remove(*id, true);
                    }
                } else {
                    // the list only (parts of unfinished downloads are cleaned up)
                    ui2.ctx.mgr.remove_many(&ids);
                }
                ui2.sel.borrow_mut().ids.clear();
                if let Some(w) = ui2.ctx.main.upgrade() {
                    w.set_selected_id(-1);
                }
                refresh(&ui2);
            });
        }
    }
}

pub(super) fn delete_download(ctx: &Ctx, id: u64) {
    let Some(v) = ctx.mgr.get(id) else { return };
    let text = if v.status == Status::Completed {
        tf("Remove “{}” from the download list?", &[&v.file_name])
    } else {
        tf("Delete the unfinished download “{}”?\nThe partly downloaded data will be removed.", &[&v.file_name])
    };
    let ctx2 = ctx.clone();
    ask_remove(ctx, &t("Delete download"), &text, move |delete_files| {
        // its progress window goes away with it
        ctx2.mgr.remove(id, delete_files);
        if let Some(w) = ctx2.main.upgrade() {
            if w.get_selected_id() == id as i32 {
                w.set_selected_id(-1);
            }
        }
    });
}

/// Removing downloads from the list: the question (with "delete the files
/// from the disk too" and "don't show this again"), or straight away when
/// the user said not to ask. `remove` gets whether files go too.
fn ask_remove(ctx: &Ctx, title: &str, text: &str, remove: impl FnOnce(bool) + 'static) {
    let s = ctx.mgr.settings();
    if !s.ask_before_remove {
        // without the question only the list entries go: files are never
        // deleted from the disk without the user seeing it right then
        remove(false);
        return;
    }
    let mgr = ctx.mgr.clone();
    confirm_with(ctx, title, text, Some(s.remove_deletes_files), move |delete_files, dont_ask| {
        let mut s = mgr.settings();
        if s.remove_deletes_files != delete_files || s.ask_before_remove == dont_ask {
            s.remove_deletes_files = delete_files;
            s.ask_before_remove = !dont_ask;
            let _ = mgr.set_settings(s);
        }
        remove(delete_files);
    });
}

pub(crate) fn reveal_in_folder(v: &DownloadView) {
    let dir = v.save_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        use windows_sys::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
        if v.status == Status::Completed && v.save_path.exists() {
            let wide: Vec<u16> = v.save_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            unsafe {
                let co = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
                let pidl = ILCreateFromPathW(wide.as_ptr());
                let ok = if !pidl.is_null() {
                    let hr = SHOpenFolderAndSelectItems(pidl, 0, std::ptr::null(), 0);
                    ILFree(pidl);
                    hr >= 0
                } else {
                    false
                };
                if co >= 0 {
                    CoUninitialize();
                }
                if ok {
                    return;
                }
            }
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = open::that_detached(&dir);
}

/// Open what is already downloaded of a video / song in the player.
pub(crate) fn play_while_downloading(ui: &Ui, id: u64) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    let kind = kind_for(&v.file_name);
    if kind != "video" && kind != "audio" {
        return message(&t("Only videos and sound files can be played while they download."));
    }
    match ui.ctx.mgr.preview_source(id) {
        None if v.kind == DownloadKind::Hls => {
            message(&t("This video comes in many small pieces that are joined at the end: it can be played when the download is done."))
        }
        None => message(&t("Nothing to play yet: wait until a part of the file is downloaded, then try again.")),
        Some(swiftget_engine::PreviewSource::File { path, .. }) => {
            if let Err(e) = preview::play(&path, &v.file_name) {
                message(&tf("Could not open the player: {}", &[&e]));
            }
        }
        Some(swiftget_engine::PreviewSource::Pair { video, video_ready, audio, audio_ready }) => {
            preview::play_pair(video, video_ready, audio.map(|a| (a, audio_ready)), &v.file_name, |err| {
                if let Some(e) = err {
                    let _ = slint::invoke_from_event_loop(move || message(&tf("Could not open the player: {}", &[&e])));
                }
            });
        }
    }
}

/// Open a finished download. A .torrent file opens in our own torrent
/// window (what it shares is the real download).
pub(crate) fn open_download(path: &Path) {
    if path.extension().map(|e| e.eq_ignore_ascii_case("torrent")).unwrap_or(false) && path.is_file() {
        torrent_ui::open_source(&path.display().to_string());
    } else {
        // the program that opens it may take its time (or Windows asks which
        // one to use): the windows don't wait for it
        let _ = open::that_detached(path);
    }
}
