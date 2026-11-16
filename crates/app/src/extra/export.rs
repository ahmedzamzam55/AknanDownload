use crate::i18n::{t, tf};
use crate::*;
use swiftget_engine::ef2::{self, Ef2Item};

pub(crate) fn export_list(mgr: &Arc<Manager>, kind: &str) {
    let ef2 = kind == "ef2";
    let Some(path) = crate::winfx::file_box()
        .set_file_name(if ef2 { "aknan-download-list.ef2" } else { "aknan-download-links.txt" })
        .add_filter(t(if ef2 { "Download list" } else { "Text file" }), &[if ef2 { "ef2" } else { "txt" }])
        .save_file()
    else {
        return;
    };
    let text = if ef2 {
        let items: Vec<Ef2Item> = mgr
            .snapshot()
            .into_iter()
            .map(|v| Ef2Item {
                url: v.parent_page.clone().filter(|_| v.kind == DownloadKind::Dash).unwrap_or(v.url.clone()),
                referer: v.headers.referer.clone(),
                cookies: v.headers.cookies.clone(),
                user_agent: v.headers.user_agent.clone(),
                file_name: Some(v.file_name.clone()),
                description: v.description.clone(),
            })
            .collect();
        ef2::write(&items)
    } else {
        mgr.all_urls().join("\r\n")
    };
    if let Err(e) = std::fs::write(&path, text) {
        message(&tf("Could not save the list: {}", &[&e]));
    }
}

pub(crate) fn import_list(ui: &Ui, kind: &str) {
    let ef2 = kind == "ef2";
    let Some(path) = crate::winfx::file_box()
        .add_filter(t(if ef2 { "Download list" } else { "Text file" }), &[if ef2 { "ef2" } else { "txt" }])
        .pick_file()
    else {
        return;
    };
    let text = match std::fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return message(&tf("Could not read the file: {}", &[&e])),
    };
    let items = ef2::parse(&text);
    if items.is_empty() {
        return message(&t("No download addresses were found in this file."));
    }
    let mut added = 0;
    let group = Some(ui.ctx.mgr.new_group());
    for it in items {
        if ui.ctx.mgr.find_url(&it.url).is_some() {
            continue;
        }
        ui.ctx.mgr.add(NewDownload {
            group,
            url: it.url,
            file_name: it.file_name,
            headers: RequestHeaders {
                referer: it.referer,
                cookies: it.cookies,
                user_agent: it.user_agent,
                ..Default::default()
            },
            description: it.description,
            queue: Some(MAIN_QUEUE),
            start: false,
            ..Default::default()
        });
        added += 1;
    }
    refresh(ui);
    message(&tf(
        "{} download(s) were added to the main download queue.\nStart them with Start Queue, or one by one with Resume.",
        &[&added],
    ));
}
