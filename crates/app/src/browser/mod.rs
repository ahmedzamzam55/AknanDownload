//! "Add Aknan Download to my browser": opens the browser on the right page
//! (its store page once the extension is published there, otherwise its
//! extensions page with the folder path ready in the clipboard).

mod detect;
mod files;
mod firefox;

pub(crate) use detect::Browser;
pub(crate) use files::{extension_folders, firefox_addon_id, prepare_extension_folders, repair_extension};
pub(crate) use firefox::update_firefox_addon;

use crate::i18n::{t, tf};
use crate::*;

pub(crate) fn open(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let list = detect::detect();
    let rows: Vec<BrowserRow> = list
        .iter()
        .map(|b| BrowserRow {
            name: b.name.into(),
            tint: b.tint,
            installed: b.exe.is_some(),
            action: t(if b.store.is_empty() { "Click to add" } else { "Click to add from the store" }).into(),
        })
        .collect();
    *ui.browser_list.borrow_mut() = list;
    d.set_browsers(ModelRc::new(VecModel::from(rows)));
    d.set_steps(SharedString::new());
    d.set_folder(files::extension_dir(false).map(|p| p.display().to_string()).unwrap_or_default().into());
    open_window!(d);
    d.window().set_minimized(false);
}

fn launch(exe: &Path, url: &str) {
    let _ = std::process::Command::new(exe).arg(url).spawn();
}

pub(super) fn open_unpacked_extension(b: &Browser, exe: &Path, d: &BrowserDialog) {
    let Some(dir) = files::extension_dir(b.firefox) else {
        d.set_steps(t("The extension folder was not found next to AknanDownload.exe. Install Aknan Download again.").into());
        return;
    };
    let path = dir.display().to_string();
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(path.clone());
    }
    launch(exe, b.extensions_page);
    d.set_folder(path.into());
    let steps = if b.firefox {
        t("Firefox opened its add-ons debugging page.\n1. Click \"Load Temporary Add-on…\".\n2. Paste the folder path (Ctrl+V, it is already copied) and open the file \"manifest.json\" in it.\n\nFirefox removes add-ons loaded this way when it closes. To keep it for good the extension must be signed by Mozilla (free) – ask for the signed version.")
    } else {
        tf("{} opened its extensions page.\n1. Turn on \"Developer mode\" (a switch on that page).\n2. Click \"Load unpacked\".\n3. Paste the folder path (Ctrl+V – it is already copied) and click \"Select folder\".\n\nThat's all: the Aknan Download icon appears next to the address bar. You do this only once.", &[&b.name])
    };
    d.set_steps(steps.into());
}

fn add(ui: &Ui, i: usize) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let Some(b) = ui.browser_list.borrow().get(i).cloned() else { return };
    let Some(exe) = b.exe.clone() else {
        d.set_steps(tf("{} is not installed on this computer.", &[&b.name]).into());
        return;
    };
    if !b.store.is_empty() {
        launch(&exe, b.store);
        d.set_steps(tf("{} opened the store page: click \"Add\" / \"Get\" there.", &[&b.name]).into());
        return;
    }
    if b.firefox {
        firefox::add_firefox(ui, &b, &exe, &d);
        return;
    }
    open_unpacked_extension(&b, &exe, &d);
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_add(move |i| add(&ui, i.max(0) as usize));
    }
    {
        let w = d.as_weak();
        d.on_copy_folder(move || {
            if let Some(d) = w.upgrade() {
                if let Ok(mut c) = arboard::Clipboard::new() {
                    let _ = c.set_text(d.get_folder().to_string());
                }
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_open_folder(move || {
            if let Some(d) = w.upgrade() {
                let f = d.get_folder().to_string();
                if !f.is_empty() {
                    let _ = open::that_detached(f);
                }
            }
        });
    }
    d.on_test_link(|| crate::message(&link_report()));
    {
        d.on_repair(move || {
            let ok = repair_extension();
            crate::message(&if ok {
                t("The add-in's files were put back. It starts again by itself within a minute (or click Reload on it in the browser's extensions page).")
            } else {
                t("Some files of the add-in could not be written. Close the browser and click \"Repair the add-in\" again.")
            });
        });
    }
    {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}

/// "Test the link": what works between the browsers and the program, and
/// what to do about what does not.
pub(crate) fn link_report() -> String {
    use std::sync::atomic::Ordering;
    let now = crate::api::now_secs();
    let ago = |at: u64| at > 0 && now.saturating_sub(at) < 15 * 60;
    let mark = |ok: bool| if ok { "✔" } else { "✖" };
    let mut lines = vec![t("Link between the browsers and Aknan Download:"), String::new()];
    let main = crate::api::MAIN_PORT.load(Ordering::Relaxed);
    lines.push(format!("{} {}", mark(main != 0), t("The program answers the add-in")));
    if !crate::api::FRONT_OPEN.load(Ordering::Relaxed) {
        lines.push(format!("• {}", t("Its usual port is kept by another program or by Windows: the add-in uses the program's own port (nothing to do).")));
    }
    let (helper, browsers) = crate::nativehost::report();
    lines.push(format!("{} {}", mark(helper), t("The browsers' helper is in place")));
    for (name, ok) in &browsers {
        lines.push(format!("   {} {}", mark(*ok), tf("{} knows the helper", &[name])));
    }
    let seen = crate::api::ADDIN_SEEN.load(Ordering::Relaxed);
    let helped = crate::api::HELPER_SEEN.load(Ordering::Relaxed);
    lines.push(format!("{} {}", mark(ago(seen)), t("An add-in talked to the program in the last 15 minutes")));
    lines.push(format!("{} {}", mark(ago(helped)), t("...through the browsers' helper")));
    lines.push(String::new());
    if !ago(seen) {
        lines.push(t("No add-in talked lately: open the browser, make sure the Aknan Download add-in is on (Extensions page), then click a download link and test again."));
    } else if !ago(helped) {
        lines.push(t("The add-in talks through the port only. In Firefox: the add-in's window > \"Connect directly\". In other browsers: reload the add-in once (Extensions page)."));
    } else {
        lines.push(t("Everything works."));
    }
    lines.join("\n")
}
