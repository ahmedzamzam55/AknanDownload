use super::format::menu_done_from;
use super::menubar::open_top_menu;
use super::modal::message;
use super::rows::refresh;
use super::shortcuts::about;
use super::state::Ui;
use crate::i18n::tf;
use crate::{browser, clipui, extra, grab, playlist, scheduler, torrent_ui, update, MainWindow};
use slint::ComponentHandle;
use swiftget_engine::default_data_dir;

pub(super) fn wire_main_menu(main: &MainWindow, ui: &Ui) {
    let mgr = ui.ctx.mgr.clone();
    {
        let ui = ui.clone();
        main.on_open_top_menu(move |idx, x, y| open_top_menu(&ui, idx, x, y));
    }
    {
        let ui = ui.clone();
        main.on_start_queue(move |q| {
            ui.ctx.mgr.start_queue(q as u64);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_stop_queue(move |q| {
            ui.ctx.mgr.stop_queue(q as u64);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_new_queue(move || scheduler::new_queue(&ui));
    }
    {
        let ui = ui.clone();
        main.on_edit_queue(move |q| scheduler::open(&ui, Some(q as u64)));
    }
    {
        let ui = ui.clone();
        main.on_delete_queue(move |q| scheduler::delete_queue(&ui, q as u64));
    }
    {
        let mgr = mgr.clone();
        main.on_open_download_folder(move || {
            let dir = mgr.settings().download_dir;
            let _ = std::fs::create_dir_all(&dir);
            let _ = open::that_detached(&dir);
        });
    }
    {
        let mgr = mgr.clone();
        main.on_export_list(move |kind| extra::export_list(&mgr, kind.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_import_list(move |kind| extra::import_list(&ui, kind.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_set_all_done(move |i| {
            ui.ctx.mgr.set_when_all_done(menu_done_from(i));
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_all_done(i);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_open_columns(move || extra::open_columns(&ui));
    }
    {
        let ui = ui.clone();
        main.on_add_category(move || extra::open_category(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_category_props(move |name| extra::open_category(&ui, Some(name.to_string())));
    }
    {
        let ui = ui.clone();
        main.on_delete_category(move |name| extra::delete_category(&ui, name.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_find(move || extra::open_find(&ui));
    }
    {
        let ui = ui.clone();
        main.on_find_next(move || extra::find_next(&ui));
    }
    {
        let ui = ui.clone();
        main.on_grabber(move || grab::open_grabber(&ui));
    }
    {
        let ui = ui.clone();
        main.on_clip_part(move || clipui::open(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_tips(move || extra::show_tip(&ui, true));
    }
    {
        let ui = ui.clone();
        main.on_playlist(move || playlist::open(&ui));
    }
    {
        let ui = ui.clone();
        main.on_torrent(move || torrent_ui::open(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_browsers(move || browser::open(&ui));
    }
    let (tools_client, tools_rt, tools_dir) = (mgr.client(), ui.ctx.rt.clone(), default_data_dir());
    main.on_update_tools(move || {
        // ffmpeg too, right now (removing the stamps makes it check)
        let _ = std::fs::remove_file(tools_dir.join("ffmpeg-checked"));
        tools_rt.spawn(swiftget_engine::tools::update_all(tools_client.clone(), tools_dir.clone()));
        std::thread::spawn(|| {
            let text = match swiftget_engine::ytdlp::update_now() {
                Ok(out) => tf(
                    "Aknan Download {}\n\nThe video downloader (yt-dlp) is up to date.\n\n{}",
                    &[&env!("CARGO_PKG_VERSION"), &out],
                ),
                Err(e) => tf("Could not update yt-dlp:\n\n{}", &[&format!("{e:#}")]),
            };
            let _ = slint::invoke_from_event_loop(move || message(&text));
        });
    });
    {
        let ui2 = ui.clone();
        main.on_check_app_update(move || update::check_now(&ui2));
    }
    if let Some(d) = ui.updater.upgrade() {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
    main.on_about(about);
    main.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
}
