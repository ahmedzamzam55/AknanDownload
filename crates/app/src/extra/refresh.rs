use crate::*;

/// http:// or https:// with a host: safe to hand to the browser.
pub(crate) fn is_web_address(s: &str) -> bool {
    url::Url::parse(s.trim())
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()))
        .unwrap_or(false)
}

pub(crate) fn open_refresh(ui: &Ui, id: u64) {
    let (Some(d), Some(v)) = (ui.refresh_dlg.upgrade(), ui.ctx.mgr.get(id)) else { return };
    if v.status == Status::Completed {
        return;
    }
    ui.ctx.mgr.pause(id);
    ui.refresh_id.set(Some(id));
    d.set_file_name(v.file_name.clone().into());
    d.set_page(v.parent_page.clone().unwrap_or_default().into());
    d.set_waiting(true);
    d.set_new_url(SharedString::new());
    open_window!(d);
}

/// The browser sent a download while we were waiting for a new address.
pub(crate) fn refresh_received(ui: &Ui, id: u64, url: &str, headers: RequestHeaders) {
    ui.refresh_id.set(None);
    if let Some(d) = ui.refresh_dlg.upgrade() {
        let _ = d.hide();
    }
    ui.ctx.mgr.set_url(id, url, Some(headers));
    ui.ctx.mgr.start(id);
    open_progress(ui, id);
    refresh(ui);
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.refresh_dlg.upgrade() else { return };
    {
        let w = d.as_weak();
        d.on_open_page(move || {
            if let Some(d) = w.upgrade() {
                let page = d.get_page().to_string();
                // only a web page: the address came from the browser or
                // a list file and must never start a program
                if is_web_address(&page) {
                    let _ = open::that_detached(page.trim());
                }
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_use_url(move || {
            let Some(d) = ui.refresh_dlg.upgrade() else { return };
            let url = d.get_new_url().trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return;
            }
            if let Some(id) = ui.refresh_id.get() {
                refresh_received(&ui, id, &url, ui.ctx.mgr.get(id).map(|v| v.headers).unwrap_or_default());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_stop_waiting(move || {
            ui.refresh_id.set(None);
            if let Some(d) = ui.refresh_dlg.upgrade() {
                let _ = d.hide();
            }
        });
    }
    {
        let ui = ui.clone();
        d.window().on_close_requested(move || {
            ui.refresh_id.set(None);
            slint::CloseRequestResponse::HideWindow
        });
    }
}

#[cfg(test)]
mod tests {
    use super::is_web_address;

    #[test]
    fn only_web_pages_are_opened() {
        assert!(is_web_address("https://example.com/page"));
        assert!(is_web_address(" http://example.com "));
        for bad in [
            "C:\\Windows\\System32\\calc.exe",
            "\\\\server\\share\\x.exe",
            "file:///C:/x.exe",
            "javascript:alert(1)",
            "ms-settings:",
            "https://",
            "",
        ] {
            assert!(!is_web_address(bad), "{bad}");
        }
    }
}
