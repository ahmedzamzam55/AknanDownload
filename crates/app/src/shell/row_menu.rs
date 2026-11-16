use super::actions::{delete_download, open_download, open_with, play_while_downloading, reveal_in_folder};
use super::add_open::{show_progress_for_start, with_extension_of};
use super::format::{kind_for, state_of};
use super::modal::{ask, message};
use super::selection::targets;
use super::state::Ui;
use crate::i18n::{self, t, tf};
use crate::{clipboard, clipui, extra, winfx, winmenu, Theme};
use slint::{ComponentHandle, Model};
use std::path::Path;
use std::time::Duration;
use swiftget_engine::Status;

/// The right-click menu of a download, as a Windows menu: the same lines as
/// the program's own one, those that can't be used now greyed out.
pub(super) fn row_menu(ui: &Ui, id: i32) {
    use winmenu::Item;
    let Some(main) = ui.ctx.main.upgrade() else { return };
    let Some(v) = ui.ctx.mgr.get(id as u64) else { return };
    let n = targets(ui, id as u64).len();
    let state = state_of(v.status);
    let done = state == 3;
    let media = matches!(kind_for(&v.file_name), "video" | "audio");
    const ACTIONS: [&str; 14] =
        ["", "openwith", "preview", "clip", "folder", "resume", "stop", "restart", "refresh", "move", "copy", "sha256", "properties", "delete"];
    let mut items = Vec::new();
    if n > 1 {
        items.push(Item::new(900, tf("{} downloads selected", &[&n]), false));
    }
    items.push(Item { bold: true, ..Item::new(100, if done { t("Open") } else { t("Show download progress") }, true) });
    items.push(Item::new(1, t("Open with…"), done));
    if !done && media {
        items.push(Item::new(2, t("Play while downloading"), true));
    }
    if media && n <= 1 {
        items.push(Item::new(3, t("Download a part of it…"), true));
    }
    items.push(Item::new(4, t("Open folder"), true));
    items.push(Item::separator());
    items.push(Item::new(5, t("Resume download"), state == 2 || state == 4 || n > 1));
    items.push(Item::new(6, t("Stop download"), state == 0 || state == 1 || n > 1));
    items.push(Item::new(7, t("Redownload"), true));
    items.push(Item::new(8, t("Refresh download address…"), !done));
    items.push(Item::new(9, t("Move / Rename…"), true));
    items.push(Item::separator());
    let mut queues: Vec<Item> = (0..ui.queues.row_count())
        .filter_map(|i| ui.queues.row_data(i))
        .map(|q| Item { checked: v.queue == Some(q.id as u64), ..Item::new(1000 + q.id.max(0) as u32, q.name.to_string(), true) })
        .collect();
    queues.push(Item::separator());
    queues.push(Item::new(999, t("Delete from queue"), v.queue.is_some()));
    items.push(Item { sub: queues, ..Item::new(0, t("Move to queue"), true) });
    items.push(Item::separator());
    items.push(Item::new(10, t("Copy address"), true));
    items.push(Item::new(11, t("Check file (SHA-256)"), done));
    items.push(Item::new(12, t("Properties"), true));
    items.push(Item::separator());
    items.push(Item::new(13, t("Remove"), true));
    items.push(Item::separator());
    items.push(Item::new(901, t("Select all"), true));
    let dark = main.global::<Theme>().get_dark();
    let picked = winmenu::popup(main.window(), &items, dark, i18n::is_rtl());
    // A click that closed the menu may reach the list a moment later without
    // its release: the list would think the button is held. Once Windows says
    // it is not, the list lets go of it.
    for wait in [150, 450] {
        let weak = ui.ctx.main.clone();
        slint::Timer::single_shot(Duration::from_millis(wait), move || {
            if let Some(m) = weak.upgrade() {
                if !winfx::left_button_down() {
                    let _ = m.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerExited);
                }
            }
        });
    }
    let Some(picked) = picked else { return };
    match picked {
        100 => main.invoke_row_activated(id),
        901 => main.invoke_select_all(),
        999 => main.invoke_row_to_queue(id, -1),
        q if q >= 1000 => main.invoke_row_to_queue(id, (q - 1000) as i32),
        a if (a as usize) < ACTIONS.len() && a > 0 => main.invoke_row_action(id, ACTIONS[a as usize].into()),
        _ => {}
    }
}

pub(super) fn row_action(ui: &Ui, id: u64, action: &str) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    match action {
        "open" => {
            open_download(&v.save_path);
        }
        "folder" => reveal_in_folder(&v),
        "openwith" => open_with(&v.save_path),
        "preview" => play_while_downloading(ui, id),
        // the page of the video (its links expire), else its address
        "clip" => clipui::open(ui, Some(v.parent_page.clone().filter(|p| p.starts_with("http")).unwrap_or_else(|| v.url.clone()))),
        "resume" => {
            ui.ctx.mgr.start(id);
            show_progress_for_start(ui, id);
        }
        "stop" => ui.ctx.mgr.pause(id),
        "restart" => {
            if ask(&tf("Download “{}” again from the beginning?", &[&v.file_name])) {
                ui.ctx.mgr.restart(id);
            }
        }
        "refresh" => extra::open_refresh(ui, id),
        "copy" => {
            clipboard::ignore_text(&v.url);
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(v.url.clone());
            }
        }
        "move" => {
            if v.status == Status::Completed {
                let mut dlg = winfx::file_box().set_file_name(&v.file_name);
                if let Some(dir) = v.save_path.parent() {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(to) = dlg.save_file() {
                    let to = {
                        let n = to.file_name().map(|n| with_extension_of(&n.to_string_lossy(), &v.file_name)).unwrap_or_default();
                        to.with_file_name(n)
                    };
                    // on its own thread: a move to another disk is a copy
                    let mgr = ui.ctx.mgr.clone();
                    std::thread::spawn(move || {
                        // the save window asked already before a file was replaced
                        if let Err(e) = mgr.move_file(id, &to, true) {
                            let text = tf("Could not move the file:\n\n{}", &[&format!("{e:#}")]);
                            let _ = slint::invoke_from_event_loop(move || message(&text));
                        }
                    });
                }
            } else {
                extra::open_properties(ui, id);
            }
        }
        "sha256" => {
            let path = v.save_path.clone();
            let name = v.file_name.clone();
            std::thread::spawn(move || {
                let text = match sha256_of(&path) {
                    Ok(hash) => {
                        let shown = tf("SHA-256 of {}:\n\n{}\n\n(copied to the clipboard)", &[&name, &hash]);
                        if let Ok(mut c) = arboard::Clipboard::new() {
                            let _ = c.set_text(hash);
                        }
                        shown
                    }
                    Err(e) => tf("Could not read the file: {}", &[&e]),
                };
                let _ = slint::invoke_from_event_loop(move || message(&text));
            });
        }
        "properties" => extra::open_properties(ui, id),
        "delete" => delete_download(&ui.ctx, id),
        _ => {}
    }
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
