use crate::i18n::{t, tf};
use crate::*;
use swiftget_engine::config::CategoryDef;

pub(crate) fn open_category(ui: &Ui, name: Option<String>) {
    let Some(d) = ui.category.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    *ui.editing_category.borrow_mut() = name.clone();
    d.set_message(SharedString::new());
    match name.as_deref() {
        None => {
            d.set_is_new(true);
            d.set_builtin(false);
            d.set_name(SharedString::new());
            d.set_exts(SharedString::new());
            d.set_sites(SharedString::new());
            d.set_folder(SharedString::new());
        }
        Some("General") => {
            d.set_is_new(false);
            d.set_builtin(true);
            d.set_name("General".into());
            d.set_exts("(every other file type)".into());
            d.set_sites(SharedString::new());
            d.set_folder(s.category_dirs.get("General").map(|p| p.display().to_string()).unwrap_or_default().into());
        }
        Some(n) => {
            let Some(c) = s.categories.iter().find(|c| c.name == n) else { return };
            d.set_is_new(false);
            d.set_builtin(c.builtin);
            d.set_name(c.name.clone().into());
            d.set_exts(c.exts.join(" ").into());
            d.set_sites(c.sites.join(" ").into());
            d.set_folder(s.category_dirs.get(n).map(|p| p.display().to_string()).unwrap_or_default().into());
        }
    }
    open_window!(d);
}

fn save_category(ui: &Ui) {
    let Some(d) = ui.category.upgrade() else { return };
    let mut s = ui.ctx.mgr.settings();
    let editing = ui.editing_category.borrow().clone();
    let name = d.get_name().trim().to_string();
    if name.is_empty() {
        d.set_message(t("Give the category a name").into());
        return;
    }
    let taken = name.eq_ignore_ascii_case("General")
        || s.categories.iter().any(|c| c.name.eq_ignore_ascii_case(&name) && Some(&c.name) != editing.as_ref());
    if taken && editing.as_deref() != Some("General") {
        d.set_message(t("A category with this name already exists").into());
        return;
    }
    let exts: Vec<String> = d
        .get_exts()
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|e| e.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty() && !e.starts_with('('))
        .collect();
    let sites: Vec<String> = d.get_sites().split_whitespace().map(|x| x.to_lowercase()).collect();
    let folder = d.get_folder().trim().to_string();
    match editing.as_deref() {
        Some("General") => {}
        Some(old) => {
            if let Some(c) = s.categories.iter_mut().find(|c| c.name == old) {
                c.name = name.clone();
                c.exts = exts.clone();
                c.sites = sites;
            }
            // a type belongs to one category only
            for c in s.categories.iter_mut().filter(|c| c.name != name) {
                c.exts.retain(|e| !exts.contains(e));
            }
            if old != name {
                if let Some(p) = s.category_dirs.remove(old) {
                    s.category_dirs.insert(name.clone(), p);
                }
            }
        }
        None => {
            for c in s.categories.iter_mut() {
                c.exts.retain(|e| !exts.contains(e));
            }
            s.categories.push(CategoryDef { name: name.clone(), exts, sites, builtin: false });
        }
    }
    if folder.is_empty() {
        s.category_dirs.remove(&name);
    } else {
        s.category_dirs.insert(name.clone(), PathBuf::from(folder));
    }
    if let Err(e) = ui.ctx.mgr.set_settings(s) {
        d.set_message(format!("{e:#}").into());
        return;
    }
    ui.ctx.mgr.recategorize();
    let _ = d.hide();
    refresh(ui);
}

pub(crate) fn delete_category(ui: &Ui, name: &str) {
    let mut s = ui.ctx.mgr.settings();
    let Some(c) = s.categories.iter().find(|c| c.name == name) else { return };
    if c.builtin || name == "General" {
        message(&t("This category comes with Aknan Download and can't be deleted (you can change its file types in Properties)."));
        return;
    }
    if !ask(&tf("Delete the category “{}”?\nIts files stay where they are and move to their file type's category.", &[&name])) {
        return;
    }
    s.categories.retain(|c| c.name != name);
    s.category_dirs.remove(name);
    let _ = ui.ctx.mgr.set_settings(s);
    ui.ctx.mgr.recategorize();
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_view("all".into());
    }
    refresh(ui);
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.category.upgrade() else { return };
    {
        let w = d.as_weak();
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = crate::winfx::file_box().pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_save(move || save_category(&ui));
    }
    {
        let w = d.as_weak();
        d.on_cancel(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}
