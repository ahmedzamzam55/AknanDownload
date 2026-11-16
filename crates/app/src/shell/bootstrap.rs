use super::activation::{acquire_instance, cli_download, init_settings_and_token};
use super::add::cancel_early;
use super::api_events::start_api;
use super::log::{elevated, install_crash_log, note};
use super::loops::{spawn_clipboard, start_timers};
use super::rows::refresh;
use super::ui_init::build_ui;
use super::window::{restart_now, saved_size, should_restart};
use crate::{api, browser, extra, preview, torrent_ui, winfx, winmenu, Theme};
use slint::{ComponentHandle, Timer};
use std::time::Duration;
use swiftget_engine::{default_data_dir, Manager, Settings};

pub(crate) fn entry() {
    install_crash_log();
    let data_dir = default_data_dir();
    let gpu = Settings::load(&data_dir.join("settings.json")).gpu_drawing;
    // The windows are drawn by the processor unless the user chose the
    // graphics card: drawing with it needs a working OpenGL driver (without
    // one the program closed by itself), takes hundreds of MB on some cards,
    // and the progress window answered the mouse in the wrong place on some
    // Windows 10 computers.
    if std::env::var_os("SLINT_BACKEND").is_none() && !gpu {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    // every window is made away from the screen and comes in once drawn
    // (no white box on the left before a window opens)
    #[cfg(windows)]
    {
        let mut pick = slint::BackendSelector::new().with_winit_window_attributes_hook(winfx::made_away);
        if std::env::var_os("SLINT_BACKEND").is_none() {
            // drawn with the graphics card: still the winit windows
            pick = pick.backend_name("winit".into());
        }
        let _ = pick.select();
    }
    if let Err(e) = run() {
        let text = format!("{e:#}");
        note(&format!("could not start: {text}"));
        if gpu {
            // the graphics card could not draw: the next start uses the processor
            let path = data_dir.join("settings.json");
            let mut s = Settings::load(&path);
            s.gpu_drawing = false;
            let _ = s.save(&path);
        }
        let _ = rfd::MessageDialog::new()
            .set_title("Aknan Download")
            .set_level(rfd::MessageLevel::Error)
            .set_description(format!(
                "Aknan Download could not start:\n\n{text}\n\nتعذّر تشغيل Aknan Download. أعد تشغيل الجهاز ثم جرّب مرة أخرى."
            ))
            .show();
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    // started by Windows at sign-in: stay next to the clock, no window
    // --from-browser: opened by the add-in's helper for a download (the
    // download's own window comes; the list stays in the tray)
    let background = std::env::args().any(|a| a == "--background" || a == "--from-browser");
    let data_dir = default_data_dir();
    // the secret key of the browser add-in: made once, kept (encrypted) in
    // the settings; another program on the computer can't talk to the port
    // without it
    let (port, token) = init_settings_and_token(&data_dir);
    let _ = api::TOKEN.set(token.clone());
    if std::env::args().any(|a| a == "--quit") {
        // run by the setup: the open copy closes (saving everything), this one too
        api::notify_quit(port);
        return Ok(());
    }
    note(&format!(
        "start{}{}",
        if background { " (with Windows)" } else { "" },
        if elevated() { " (as administrator)" } else { "" }
    ));

    let Some(inst) = acquire_instance(&data_dir, port) else {
        return Ok(());
    };
    let _ = &token;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let mgr = Manager::new(rt.handle().clone(), data_dir.clone())?;
    // yt-dlp and ffmpeg: put back if missing, update silently once a day
    swiftget_engine::tools::spawn_updater(rt.handle(), mgr.client(), data_dir.clone());

    let (main, ui, has_tray) = build_ui(&mgr, &rt)?;
    let ctx_exit = ui.ctx.clone();

    // ------------------------------------------------------------ browser + clipboard
    start_api(&ui, inst.server, inst.main_server);
    spawn_clipboard(&mgr, &ui.ctx);

    // ------------------------------------------------------------ refresh loops
    let (fast_timer, timer, power_timer) = start_timers(&ui);
    refresh(&ui);

    {
        let mgr = mgr.clone();
        main.window().on_close_requested(move || {
            // with the icon next to the clock, closing only hides the window
            if !(has_tray && mgr.settings().close_to_tray) {
                let _ = slint::quit_event_loop();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    // after an update: what is new, once, in the language of the program
    let mut just_updated = false;
    {
        let mut s = mgr.settings();
        let now = env!("CARGO_PKG_VERSION");
        if s.last_version != now {
            // an empty last_version on a copy used before = an update from 1.0.3
            let updated = if s.last_version.is_empty() { s.extension_prompted } else { true };
            s.last_version = now.into();
            let _ = mgr.set_settings(s);
            just_updated = updated;
            if updated {
                let ui = ui.clone();
                Timer::single_shot(Duration::from_millis(1500), move || extra::whats_new(&ui));
            }
        }
    }
    // the size the user gave the main window last time
    if let Some((w, h)) = saved_size(&mgr, "main") {
        main.window().set_size(slint::LogicalSize::new(w, h));
    }
    let was_maximized = mgr.settings().main_maximized;
    if !(background && has_tray) {
        main.show()?;
        main.invoke_focus_list();
        // the menus of Windows in the look of the program from the start
        winmenu::set_dark_menus(main.global::<Theme>().get_dark());
        // the main window: in the middle of the screen (once drawn), and it
        // can be maximized; maximized there, so it comes back there
        let w = main.as_weak();
        winfx::place_then(main.as_weak(), true, move || {
            if was_maximized {
                if let Some(m) = w.upgrade() {
                    m.window().set_maximized(true);
                }
            }
        });
        winfx::set_opacity(main.window(), mgr.settings().opacity);
        let mut s = mgr.settings();
        if !s.extension_prompted {
            // first start: help adding the browser extension
            s.extension_prompted = true;
            let _ = mgr.set_settings(s);
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(800), move || browser::open(&ui));
        } else if s.show_tips && !just_updated {
            // never together with a message box (What's new, a question…)
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(600), move || extra::show_tip_when_free(ui));
        }
    }
    if let Some(src) = inst.open_torrent {
        // opened from Windows with a torrent: its window, once everything is up
        Timer::single_shot(Duration::from_millis(500), move || torrent_ui::open_source(&src));
    }
    if let Some(ask) = inst.cli_ask {
        // started with a download on the command line: once everything is up
        let ui = ui.clone();
        Timer::single_shot(Duration::from_millis(600), move || cli_download(&ui, ask));
    }
    {
        // a .torrent file dropped on the window
        winfx::keep_drawn_with_drop(&main, |path| {
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent")) && path.is_file() {
                torrent_ui::open_source(&path.display().to_string());
            }
        });
    }
    slint::run_event_loop_until_quit()?;
    note("closed");
    preview::clean_all();
    {
        // opens maximized next time when it was maximized now
        let max = main.window().is_maximized();
        let mut s = mgr.settings();
        if s.main_maximized != max {
            s.main_maximized = max;
            let _ = mgr.set_settings(s);
        }
    }

    // save everything and stop downloads cleanly (they resume next time)
    drop(timer);
    drop(fast_timer);
    drop(power_timer);
    // a download nobody pressed Start for is not kept
    cancel_early(&ctx_exit);
    mgr.shutdown();
    rt.shutdown_timeout(Duration::from_secs(2));
    if should_restart() {
        restart_now();
    }
    Ok(())
}
