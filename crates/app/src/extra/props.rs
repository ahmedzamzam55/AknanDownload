use crate::i18n::{t, tf};
use crate::*;
use swiftget_engine::DetailsEdit;

pub(crate) fn open_properties(ui: &Ui, id: u64) {
    let (Some(d), Some(v)) = (ui.props.upgrade(), ui.ctx.mgr.get(id)) else { return };
    ui.props_id.set(Some(id));
    let done = v.status == Status::Completed;
    d.set_file_name(v.file_name.clone().into());
    d.set_kind(kind_for(&v.file_name).into());
    d.set_ext(filename::extension(&v.file_name).to_uppercase().into());
    let ext = filename::extension(&v.file_name).to_uppercase();
    d.set_type_text(
        if ext.is_empty() { t(&v.category) } else { tf("{} file  ·  {}", &[&ext, &t(&v.category)]) }.into(),
    );
    d.set_status(
        match v.status {
            Status::Downloading | Status::Paused if v.total_size.is_some() => {
                format!("{} ({:.1}%)", t(v.status.label()), v.progress() * 100.0)
            }
            s => t(s.label()),
        }
        .into(),
    );
    d.set_size(
        match v.total_size {
            Some(t) => format!("{} ({} bytes)", fmt_size(t), t),
            None => t("Unknown"),
        }
        .into(),
    );
    d.set_added(fmt_time(v.added_at).into());
    d.set_last_try(v.finished_at.or(v.last_try).map(fmt_time).unwrap_or_default().into());
    d.set_result(
        match (&v.error, v.status) {
            (Some(e), _) => crate::err_text(e),
            (None, Status::Completed) => t("The file was downloaded successfully"),
            _ => String::new(),
        }
        .into(),
    );
    d.set_done(done);
    d.set_save_to(v.save_path.display().to_string().into());
    d.set_url(v.url.clone().into());
    d.set_referer(v.referer().to_string().into());
    d.set_description(v.description.clone().unwrap_or_default().into());
    d.set_checksum(v.checksum.clone().unwrap_or_default().into());
    d.set_use_login(v.headers.auth.is_some());
    d.set_user(v.headers.auth.as_ref().map(|a| a.0.clone()).unwrap_or_default().into());
    d.set_password(v.headers.auth.as_ref().map(|a| a.1.clone()).unwrap_or_default().into());
    d.set_message(SharedString::new());
    open_window!(d);
}

fn save_properties(ui: &Ui) {
    let (Some(d), Some(id)) = (ui.props.upgrade(), ui.props_id.get()) else { return };
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    let auth = (d.get_use_login() && !d.get_user().trim().is_empty())
        .then(|| (d.get_user().trim().to_string(), d.get_password().to_string()));
    let checksum = d.get_checksum().trim().to_string();
    if !checksum.is_empty() && swiftget_engine::checksum::Expected::parse(&checksum).is_none() {
        d.set_message(t("This is not a checksum: paste its MD5, SHA-1, SHA-256, SHA-384 or SHA-512 value").into());
        return;
    }
    let edit = DetailsEdit {
        url: (v.status != Status::Completed).then(|| d.get_url().trim().to_string()),
        referer: Some(d.get_referer().trim().to_string()),
        description: Some(d.get_description().trim().to_string()),
        auth,
        checksum: Some(checksum),
    };
    if let Err(e) = ui.ctx.mgr.update_details(id, edit) {
        d.set_message(format!("{e:#}").into());
        return;
    }
    let mut target = PathBuf::from(d.get_save_to().trim());
    // a name typed without its extension keeps the file's own
    if let Some(n) = target.file_name().map(|n| crate::with_extension_of(&n.to_string_lossy(), &v.file_name)) {
        target.set_file_name(n);
    }
    if !target.as_os_str().is_empty() && target != v.save_path {
        let result = if v.status == Status::Completed {
            // on its own thread: a move to another disk is a copy
            let mgr = ui.ctx.mgr.clone();
            std::thread::spawn(move || {
                if let Err(e) = mgr.move_file(id, &target, false) {
                    let text = format!("{e:#}");
                    let text = if text.contains("is already in that folder") {
                        crate::i18n::t("A file with this name is already in that folder, so nothing was moved. Choose another name.")
                    } else {
                        text
                    };
                    let _ = slint::invoke_from_event_loop(move || crate::message(&text));
                }
            });
            Ok(())
        } else {
            let dir = target.parent().map(Path::to_path_buf).unwrap_or_default();
            let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() {
                Err(anyhow::anyhow!("type a file name"))
            } else {
                ui.ctx.mgr.change_target(id, &dir, &name)
            }
        };
        if let Err(e) = result {
            d.set_message(tf("Could not change where it is saved: {}", &[&format!("{e:#}")]).into());
            return;
        }
    }
    let _ = d.hide();
    refresh(ui);
}

pub(super) fn wire(ui: &Ui) {
    let Some(d) = ui.props.upgrade() else { return };
    {
        let w = d.as_weak();
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            let current = PathBuf::from(d.get_save_to().as_str());
            let mut dlg = crate::winfx::file_box();
            if let Some(dir) = current.parent().filter(|p| p.exists()) {
                dlg = dlg.set_directory(dir);
            }
            if let Some(name) = current.file_name() {
                dlg = dlg.set_file_name(name.to_string_lossy());
            }
            if let Some(p) = dlg.save_file() {
                d.set_save_to(p.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_open_file(move || {
            if let Some(v) = ui.props_id.get().and_then(|id| ui.ctx.mgr.get(id)) {
                crate::open_download(&v.save_path);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_save(move || save_properties(&ui));
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
