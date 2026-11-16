use super::show_lists;
use crate::i18n::t;
use crate::*;
use swiftget_engine::config::{ServerLimit, SiteLogin};

pub(super) fn wire_lists(ui: &Ui, d: &SettingsDialog) {
    {
        let ui2 = ui.clone();
        d.on_server_add(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let host = d.get_server_host().trim().trim_start_matches("https://").trim_start_matches("http://").to_string();
            let host = host.split('/').next().unwrap_or("").to_string();
            if host.is_empty() {
                d.set_message(t("Type the server, e.g. example.com").into());
                return;
            }
            {
                let mut l = ui2.options_lists.borrow_mut();
                let conns = d.get_server_conns().clamp(1, 64) as u32;
                match l.servers.iter_mut().find(|s| s.host.eq_ignore_ascii_case(&host)) {
                    Some(s) => s.connections = conns,
                    None => l.servers.push(ServerLimit { host, connections: conns }),
                }
            }
            d.set_server_host(SharedString::new());
            d.set_message(SharedString::new());
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_server_remove(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let i = d.get_server_current();
            {
                let mut l = ui2.options_lists.borrow_mut();
                if i >= 0 && (i as usize) < l.servers.len() {
                    l.servers.remove(i as usize);
                }
            }
            d.set_server_current(-1);
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_pick(move |i| {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            if let Some(l) = ui2.options_lists.borrow().logins.get(i.max(0) as usize) {
                d.set_login_site(l.site.clone().into());
                d.set_login_user(l.user.clone().into());
                d.set_login_password(l.password.clone().into());
            }
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_add(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let site = d.get_login_site().trim().to_string();
            let user = d.get_login_user().trim().to_string();
            if site.is_empty() || user.is_empty() {
                d.set_message(t("Type the site and the user name").into());
                return;
            }
            let password = d.get_login_password().to_string();
            {
                let mut l = ui2.options_lists.borrow_mut();
                match l.logins.iter_mut().find(|x| x.site.eq_ignore_ascii_case(&site)) {
                    Some(x) => {
                        x.user = user;
                        x.password = password;
                    }
                    None => l.logins.push(SiteLogin { site, user, password }),
                }
            }
            d.set_login_site(SharedString::new());
            d.set_login_user(SharedString::new());
            d.set_login_password(SharedString::new());
            d.set_login_current(-1);
            d.set_message(SharedString::new());
            show_lists(&ui2, &d);
        });
    }
    {
        let ui2 = ui.clone();
        d.on_login_remove(move || {
            let Some(d) = ui2.settings_dlg.upgrade() else { return };
            let i = d.get_login_current();
            {
                let mut l = ui2.options_lists.borrow_mut();
                if i >= 0 && (i as usize) < l.logins.len() {
                    l.logins.remove(i as usize);
                }
            }
            d.set_login_current(-1);
            show_lists(&ui2, &d);
        });
    }
}
