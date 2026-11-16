use super::state::Ui;
use crate::{ColDef, Cols};
use slint::Model;
use swiftget_engine::Settings;

pub(crate) fn cols_from(s: &Settings) -> Cols {
    let c = |k: &str| s.column_visible(k);
    Cols {
        q: c("q"),
        size: c("size"),
        status: c("status"),
        eta: c("eta"),
        speed: c("speed"),
        last: c("last"),
        desc: c("desc"),
        added: c("added"),
        saveto: c("saveto"),
        referer: c("referer"),
        parent: c("parent"),
    }
}

/// Which column a point `x` of the header is over (the header starts with
/// 6 px of padding and the 32 px file type column).
pub(crate) fn column_at(widths: &[f32], x: f32) -> usize {
    let mut left = 6.0 + 32.0;
    for (i, w) in widths.iter().enumerate() {
        if x < left + w {
            return i;
        }
        left += w;
    }
    widths.len().saturating_sub(1)
}

/// Keep the header's column order and widths in the settings.
pub(super) fn save_columns(ui: &Ui) {
    let cols: Vec<ColDef> = (0..ui.columns_model.row_count()).filter_map(|i| ui.columns_model.row_data(i)).collect();
    let mut s = ui.ctx.mgr.settings();
    let mut order: Vec<String> = cols.iter().map(|c| c.key.to_string()).collect();
    // hidden columns keep their place at the end
    let rest: Vec<String> =
        s.column_order.iter().cloned().chain(swiftget_engine::config::ALL_COLUMNS.iter().map(|k| k.to_string())).collect();
    for k in rest {
        if !order.contains(&k) {
            order.push(k);
        }
    }
    s.column_order = order;
    for c in &cols {
        s.column_widths.insert(c.key.to_string(), c.width.round().clamp(36.0, 2000.0) as u32);
    }
    let _ = ui.ctx.mgr.set_settings(s);
}

/// Look of the main window from the settings (columns, toolbar, sorting).
pub(crate) fn apply_view_settings(ui: &Ui, s: &Settings) {
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_hide_categories(s.hide_categories);
        m.set_sort_by(s.sort_by.clone().into());
        m.set_sort_desc(s.sort_desc);
        m.set_cols(cols_from(s));
        let defs: Vec<ColDef> =
            s.column_layout().into_iter().map(|(key, w)| ColDef { key: key.into(), width: w as f32 }).collect();
        ui.columns_model.set_vec(defs);
        m.set_toolbar_small(s.toolbar_small);
    }
}
