use super::add_open::{open_add_dialog, show_progress_for_start};
use super::log::{end_process, note};
use super::rows::refresh;
use super::state::{Ui, VideoExtra, APP};
use crate::{api, autostart, browser, cli, i18n, nativehost};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;
use swiftget_engine::{NewDownload, RequestHeaders, Settings};

pub(super) struct StartupInstance {
    pub(super) server: Option<tiny_http::Server>,
    pub(super) main_server: Option<tiny_http::Server>,
    pub(super) open_torrent: Option<String>,
    pub(super) cli_ask: Option<cli::CliAsk>,
}

/// First-start setup values and browser add-in secret key.
pub(super) fn init_settings_and_token(data_dir: &Path) -> (u16, String) {
    let path = data_dir.join("settings.json");
    let first_start = !path.exists();
    let mut s = Settings::load(&path);
    if first_start {
        // what was chosen in the setup program: its language, and
        // whether torrents are ours
        if let Some(lang) = autostart::setup_value("Language").filter(|l| i18n::LANGS.iter().any(|(code, _)| code == l)) {
            s.language = lang;
        }
        if let Some(t) = autostart::setup_value("Torrents") {
            s.torrent_main = t != "0";
        }
        let _ = std::fs::create_dir_all(data_dir);
        let _ = s.save(&path);
    }
    if s.api_token.len() < 32 {
        s.api_token = swiftget_engine::secret::random_token();
        let _ = std::fs::create_dir_all(data_dir);
        let _ = s.save(&path);
    }
    (s.browser_port, s.api_token)
}

/// Acquire single-instance ports or forward CLI/torrent/show request to the running copy.
pub(super) fn acquire_instance(data_dir: &Path, port: u16) -> Option<StartupInstance> {
    // opened again by the program itself: the old one may still be closing
    let mut first = api::bind(port);
    if first.is_err() && std::env::args().any(|a| a == "--restarted") {
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(250));
            first = api::bind(port);
            if first.is_ok() {
                break;
            }
        }
    }
    // a .torrent file / magnet link opened with Aknan Download (double click)
    let open_torrent = std::env::args().skip(1).find(|a| api::torrent_source_ok(a));
    // a download handed over on the command line ("/d address ...")
    let cli_ask = cli::parse(std::env::args().skip(1)).filter(|c| !c.is_empty());
    // One instance only: if the port is taken by us, bring that one to front.
    let server = match first {
        Ok(s) => Some(s),
        Err(_) => {
            if let Some(src) = &open_torrent {
                if api::notify_open_torrent(port, src) {
                    return None;
                }
            }
            if let Some(ask) = &cli_ask {
                let own = Settings::load(&data_dir.join("settings.json")).api_port;
                if api::notify_cli(port, ask) || (own != 0 && api::notify_cli(own, ask)) {
                    return None;
                }
            }
            match api::notify_existing(port) {
                api::Existing::Shown => {
                    note("already open: brought to the front");
                    return None;
                }
                api::Existing::Hangs(pid) => {
                    note(&format!("already open but not answering (process {pid})"));
                    let yes = matches!(
                        rfd::MessageDialog::new()
                            .set_title(APP)
                            .set_level(rfd::MessageLevel::Warning)
                            .set_description(
                                "Aknan Download is already open but does not answer.\nClose it and open it again?\n\n\
                                 Aknan Download مفتوح بالفعل لكنه لا يستجيب.\nهل تريد إغلاقه وفتحه من جديد؟",
                            )
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show(),
                        rfd::MessageDialogResult::Yes
                    );
                    if !yes || pid == 0 || pid == std::process::id() {
                        return None;
                    }
                    end_process(pid);
                    // its port is free a moment later
                    let mut got = None;
                    for _ in 0..40 {
                        std::thread::sleep(Duration::from_millis(250));
                        if let Ok(s) = api::bind(port) {
                            got = Some(s);
                            break;
                        }
                    }
                    got
                }
                api::Existing::None if api::show_on_own_port(Settings::load(&data_dir.join("settings.json")).api_port) => {
                    // the open copy answered on its own port (the known one
                    // is kept by someone else): it comes to the front
                    note("already open (its own port): brought to the front");
                    return None;
                }
                api::Existing::None => {
                    // the known port is kept by another program, or Windows
                    // keeps it for itself (a range of ports set aside by
                    // Hyper-V / WSL / an update): the program still opens its
                    // own port and the add-in finds it there (token.json, or
                    // the port it was given when it was paired)
                    note(&format!("port {port} can't be opened (another program or Windows keeps it): the add-in uses the program's own port"));
                    None
                }
            }
        }
    };
    // the port the add-in really talks to: random on this computer (kept for
    // the next start), and the add-in folders get it with the key before
    // anything answers there (an add-in reloading right after an update
    // finds both at once)
    api::FRONT_OPEN.store(server.is_some(), Ordering::Relaxed);
    let main_server = {
        let path = data_dir.join("settings.json");
        let mut s = Settings::load(&path);
        let got = api::bind_main(s.api_port);
        if let Some((_, p)) = &got {
            api::MAIN_PORT.store(*p, Ordering::Relaxed);
            if s.api_port != *p {
                s.api_port = *p;
                let _ = s.save(&path);
            }
        }
        browser::prepare_extension_folders();
        // the browser's helper (no port in the browser): on its own thread,
        // nothing at start waits for it
        let dir = data_dir.to_path_buf();
        std::thread::spawn(move || nativehost::install(&dir));
        got.map(|(srv, _)| srv)
    };
    Some(StartupInstance { server, main_server, open_torrent, cli_ask })
}

/// A download (or "start the queue") from the command line.
pub(super) fn cli_download(ui: &Ui, ask: cli::CliAsk) {
    if let Some(url) = ask.url.clone() {
        let folder = ask.folder.as_ref().map(PathBuf::from).filter(|f| f.is_absolute());
        if let Some(f) = &folder {
            let _ = std::fs::create_dir_all(f);
        }
        if ask.quiet || ask.later {
            let id = ui.ctx.mgr.add(NewDownload {
                url,
                file_name: ask.name.clone(),
                save_dir: folder,
                queue: ask.later.then_some(swiftget_engine::config::MAIN_QUEUE),
                start: !ask.later,
                ..Default::default()
            });
            if !ask.later {
                show_progress_for_start(ui, id);
            }
            refresh(ui);
        } else {
            open_add_dialog(&ui.ctx, url, RequestHeaders::default(), ask.name.clone(), VideoExtra::default());
            // the folder asked for, with the name the window shows
            if let (Some(f), Some(d)) = (folder, ui.ctx.add.upgrade()) {
                d.set_save_folder(f.display().to_string().into());
                if let Some(n) = ask.name.clone().filter(|n| !n.trim().is_empty()) {
                    d.set_file_name(n.into());
                }
            }
        }
    }
    if ask.start_queue {
        ui.ctx.mgr.start_queue(swiftget_engine::config::MAIN_QUEUE);
        refresh(ui);
    }
}
