use super::Item;
use crate::i18n::tf;
use crate::*;

fn type_ok(it: &Item, filter: i32) -> bool {
    match filter {
        1 => it.kind == "image",
        2 => it.kind == "video",
        3 => it.kind == "audio",
        4 => matches!(it.kind, "archive" | "exe" | "apk" | "disc"),
        5 => matches!(it.kind, "pdf" | "doc" | "sheet" | "slides" | "text") && !it.is_page,
        6 => it.is_page,
        _ => !it.is_page,
    }
}

fn row(it: &Item) -> LinkRow {
    let ext = match (&it.gone, it.size) {
        (Some(_), _) => "✗".to_string(),
        (None, Some(n)) if n > 0 => format!("{}  {}", it.ext, swiftget_engine::model::fmt_size(n)),
        _ => it.ext.clone(),
    };
    LinkRow {
        checked: it.checked,
        name: it.name.clone().into(),
        rtl: crate::i18n::rtl_text(&it.name),
        kind: it.kind.into(),
        ext: ext.trim().into(),
        size: SharedString::new(),
        done: false,
        text: it.gone.clone().unwrap_or_else(|| it.text.clone()).into(),
        url: it.url.clone().into(),
    }
}

/// Rebuild the visible rows from the filters.
pub(super) fn show(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    let filter = d.get_filter().to_lowercase();
    let type_filter = d.get_type_filter();
    let mut g = ui.grab_state.borrow_mut();
    let shown: Vec<usize> = g
        .all
        .iter()
        .enumerate()
        .filter(|(_, it)| type_ok(it, type_filter))
        .filter(|(_, it)| {
            filter.is_empty() || it.name.to_lowercase().contains(&filter) || it.url.to_lowercase().contains(&filter)
        })
        .map(|(i, _)| i)
        .collect();
    let rows: Vec<LinkRow> = shown.iter().map(|i| row(&g.all[*i])).collect();
    g.shown = shown;
    let model = g.model.get_or_insert_with(|| Rc::new(VecModel::default())).clone();
    let checked = g.all.iter().filter(|i| i.checked).count();
    drop(g);
    update_model(&model, rows);
    d.set_links(ModelRc::from(model));
    d.set_checked_text(tf("{} file(s) checked", &[&checked]).into());
}

pub(super) fn reset(ui: &Ui, d: &GrabberDialog, links_mode: bool) {
    let mut g = ui.grab_state.borrow_mut();
    if let Some(c) = g.cancel.take() {
        c.cancel();
    }
    g.all.clear();
    g.shown.clear();
    g.exploring = false;
    g.progress = None;
    g.incoming = Arc::new(Mutex::new(Vec::new()));
    if let Some(c) = g.asking_cancel.take() {
        c.cancel();
    }
    g.answers = Default::default();
    g.asking = Default::default();
    drop(g);
    d.set_spa_hint(false);
    let s = ui.ctx.mgr.settings();
    d.set_links_mode(links_mode);
    d.set_exploring(false);
    d.set_message(SharedString::new());
    d.set_filter(SharedString::new());
    d.set_type_filter(0);
    d.set_queue_names(queue_names(&s));
    d.set_queue_index(0);
    d.set_folder(SharedString::new());
    d.set_progress_text(SharedString::new());
}
