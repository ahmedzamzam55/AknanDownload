//! The Options window (all its tabs).

mod fill;
mod lists;
mod read;
mod wire;

pub(crate) use wire::wire;

use crate::*;
use swiftget_engine::config::{ServerLimit, SiteLogin};

/// Lists edited inside the Options window before OK is pressed.
#[derive(Default)]
pub(crate) struct Lists {
    pub(super) servers: Vec<ServerLimit>,
    pub(super) logins: Vec<SiteLogin>,
}

pub(super) const KEYS: [&str; 4] = ["alt", "ctrl", "shift", "none"];

/// "Stop sharing once it sent": the choices of Options > Torrents (0 = no limit).
pub(super) const RATIOS: [f32; 7] = [0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0];

pub(super) fn key_index(k: &str) -> i32 {
    KEYS.iter().position(|x| *x == k).unwrap_or(3) as i32
}

pub(crate) fn open(ui: &Ui, tab: i32) {
    let Some(d) = ui.settings_dlg.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    {
        let mut l = ui.options_lists.borrow_mut();
        l.servers = s.server_limits.clone();
        l.logins = s.logins.clone();
    }
    fill::fill(ui, &d, &s);
    d.set_tab(tab);
    d.set_message(SharedString::new());
    open_window!(d);
}

pub(super) fn show_lists(ui: &Ui, d: &SettingsDialog) {
    let l = ui.options_lists.borrow();
    let servers: Vec<PairRow> = l
        .servers
        .iter()
        .map(|x| PairRow { a: x.host.clone().into(), b: x.connections.to_string().into(), c: SharedString::new() })
        .collect();
    d.set_server_rows(ModelRc::new(VecModel::from(servers)));
    let logins: Vec<PairRow> = l
        .logins
        .iter()
        .map(|x| PairRow { a: x.site.clone().into(), b: x.user.clone().into(), c: SharedString::new() })
        .collect();
    d.set_login_rows(ModelRc::new(VecModel::from(logins)));
}
