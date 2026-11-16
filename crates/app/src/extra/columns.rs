use crate::*;
use swiftget_engine::config::{ALL_COLUMNS, DEFAULT_COLUMNS};

pub(crate) fn open_columns(ui: &Ui) {
    let Some(d) = ui.columns.upgrade() else { return };
    d.set_cols(cols_from(&ui.ctx.mgr.settings()));
    open_window!(d);
}

fn cols_to_list(c: &Cols) -> Vec<String> {
    let flags = [c.q, c.size, c.status, c.eta, c.speed, c.last, c.desc, c.added, c.saveto, c.referer, c.parent];
    ALL_COLUMNS.iter().zip(flags).filter(|(_, on)| *on).map(|(k, _)| k.to_string()).collect()
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.columns.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_save(move || {
            let Some(d) = ui.columns.upgrade() else { return };
            let mut s = ui.ctx.mgr.settings();
            s.columns = cols_to_list(&d.get_cols());
            let _ = ui.ctx.mgr.set_settings(s.clone());
            apply_view_settings(&ui, &s);
            let _ = d.hide();
        });
    }
    {
        let w = d.as_weak();
        d.on_reset(move || {
            if let Some(d) = w.upgrade() {
                let s = Settings { columns: DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect(), ..Default::default() };
                d.set_cols(cols_from(&s));
            }
        });
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
