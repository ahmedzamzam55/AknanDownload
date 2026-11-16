use super::add::wire_add;
use super::batch::wire_batch;
use super::columns::apply_view_settings;
use super::complete::wire_complete;
use super::look::apply_theme;
use super::main_wire::wire_main;
use super::shortcuts::wire_about;
use super::state::{AddState, Ctx, DialogHandles, Selection, Ui};
use super::window::set_language;
use crate::{
    autostart, browser, extra, grab, i18n, options, playlist, scheduler, torrent_ui, tray, update, AboutDialog,
    AddDialog, BatchDialog, BrowserDialog, CategoryDialog, ColumnsDialog, CompleteDialog, CountdownDialog,
    DuplicateDialog, FindDialog, GrabberDialog, MainWindow, PlaylistDialog, PropertiesDialog, RefreshDialog,
    SchedulerDialog, SettingsDialog, TipDialog, TorrentDialog, UpdateDialog,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use swiftget_engine::Manager;

pub(super) fn build_ui(mgr: &Arc<Manager>, rt: &tokio::runtime::Runtime) -> anyhow::Result<(MainWindow, Ui, bool)> {
    let main = MainWindow::new()?;
    main.set_app_version(env!("CARGO_PKG_VERSION").into());
    let add = AddDialog::new()?;
    let batch = BatchDialog::new()?;
    let settings_dlg = SettingsDialog::new()?;
    let complete = CompleteDialog::new()?;
    let columns = ColumnsDialog::new()?;
    let category = CategoryDialog::new()?;
    let find = FindDialog::new()?;
    let props = PropertiesDialog::new()?;
    let dup = DuplicateDialog::new()?;
    let countdown = CountdownDialog::new()?;
    let sched = SchedulerDialog::new()?;
    let grabber = GrabberDialog::new()?;
    let tips = TipDialog::new()?;
    let refresh_dlg = RefreshDialog::new()?;
    let playlist_dlg = PlaylistDialog::new()?;
    let torrent_dlg = TorrentDialog::new()?;
    let browsers_dlg = BrowserDialog::new()?;
    let updater_dlg = UpdateDialog::new()?;
    let about_dlg = AboutDialog::new()?;
    wire_about(&about_dlg);

    let ctx = Ctx {
        mgr: mgr.clone(),
        rt: rt.handle().clone(),
        main: main.as_weak(),
        add: add.as_weak(),
        add_state: Arc::new(Mutex::new(AddState::default())),
    };
    let ui = Ui {
        ctx,
        rows: Rc::new(VecModel::default()),
        queues: Rc::new(VecModel::default()),
        cats: Rc::new(VecModel::default()),
        progress: Rc::new(RefCell::new(HashMap::new())),
        batch: batch.as_weak(),
        settings_dlg: settings_dlg.as_weak(),
        complete: complete.as_weak(),
        complete_id: Rc::new(Cell::new(None)),
        complete_folder: Rc::default(),
        groups_waiting: Rc::default(),
        columns: columns.as_weak(),
        category: category.as_weak(),
        editing_category: Rc::new(RefCell::new(None)),
        find: find.as_weak(),
        props: props.as_weak(),
        props_id: Rc::new(Cell::new(None)),
        dup: dup.as_weak(),
        pending: Rc::new(RefCell::new(None)),
        countdown: countdown.as_weak(),
        power: Rc::new(RefCell::new(None)),
        sched: sched.as_weak(),
        sched_state: Rc::new(RefCell::new(scheduler::SchedState::default())),
        grabber: grabber.as_weak(),
        grab_state: Rc::new(RefCell::new(grab::GrabUi::default())),
        tips: tips.as_weak(),
        refresh_dlg: refresh_dlg.as_weak(),
        refresh_id: Rc::new(Cell::new(None)),
        options_lists: Rc::new(RefCell::new(options::Lists::default())),
        sel: Rc::new(RefCell::new(Selection::default())),
        playlist: playlist_dlg.as_weak(),
        playlist_rows: Rc::new(VecModel::default()),
        torrent: torrent_dlg.as_weak(),
        torrent_rows: Rc::new(VecModel::default()),
        browsers: browsers_dlg.as_weak(),
        updater: updater_dlg.as_weak(),
        columns_model: Rc::new(VecModel::default()),
        browser_list: Rc::new(RefCell::new(Vec::new())),
        tray: Rc::new(RefCell::new(None)),
        _dialogs: Rc::new(DialogHandles {
            _add: add,
            _batch: batch,
            _settings: settings_dlg,
            _complete: complete,
            _columns: columns,
            _category: category,
            _find: find,
            _props: props,
            _dup: dup,
            _countdown: countdown,
            _sched: sched,
            _grabber: grabber,
            _tips: tips,
            _refresh: refresh_dlg,
            _playlist: playlist_dlg,
            _torrent: torrent_dlg,
            _browsers: browsers_dlg,
            _updater: updater_dlg,
            _about: about_dlg,
        }),
    };
    main.set_rows(ModelRc::from(ui.rows.clone()));
    main.set_columns(ModelRc::from(ui.columns_model.clone()));
    main.set_queues(ModelRc::from(ui.queues.clone()));
    main.set_categories(ModelRc::from(ui.cats.clone()));
    // Arabic or English (Options > General > Language, View > Language)
    set_language(&ui, i18n::effective(&mgr.settings().language));
    {
        // first run: start with Windows, like other download managers
        let mut s = mgr.settings();
        if !s.autostart_set {
            // the installer already asked; a copy run without it starts with Windows
            if !autostart::installed_by_setup() {
                autostart::set(true);
            }
            s.autostart_set = true;
            let _ = mgr.set_settings(s);
        }
    }
    let tray = tray::Tray::new();
    let has_tray = tray.is_some();
    *ui.tray.borrow_mut() = tray;
    {
        let s = mgr.settings();
        apply_theme(&ui, s.theme as i32);
        apply_view_settings(&ui, &s);
    }

    wire_main(&ui);
    // a new version of the program itself: from GitHub, installed silently
    update::spawn(&ui.ctx);
    wire_add(&ui);
    wire_batch(&ui);
    wire_complete(&ui);
    options::wire(&ui);
    scheduler::wire(&ui);
    grab::wire(&ui);
    extra::wire(&ui);
    playlist::wire(&ui);
    torrent_ui::wire(&ui);
    browser::wire(&ui);
    assert!(ui.all_dialogs_alive(), "all Slint dialog handles must stay alive");
    Ok((main, ui, has_tray))
}
