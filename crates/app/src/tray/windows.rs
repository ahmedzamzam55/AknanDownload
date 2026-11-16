use super::mini::{handle_menu_id, push_action, DL_PREFIX};
use super::{DownloadAction, TrayAction};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MAIN_ID: &str = "aknan-main";

pub struct Tray {
    _icon: TrayIcon,
    show: MenuItem,
    add: MenuItem,
    resume: MenuItem,
    stop: MenuItem,
    limiter: CheckMenuItem,
    sched: MenuItem,
    options: MenuItem,
    about: MenuItem,
    exit: MenuItem,
    ids: Vec<(MenuId, fn() -> TrayAction)>,
}

impl Tray {
    pub fn new() -> Option<Tray> {
        let menu = Menu::new();
        let show = MenuItem::new(crate::i18n::t("Restore Aknan Download"), true, None);
        let add = MenuItem::new(crate::i18n::t("Add new download…"), true, None);
        let resume = MenuItem::new(crate::i18n::t("Resume all downloads"), true, None);
        let stop = MenuItem::new(crate::i18n::t("Stop all downloads"), true, None);
        let limiter = CheckMenuItem::new(crate::i18n::t("Speed Limiter"), true, false, None);
        let sched = MenuItem::new(crate::i18n::t("Scheduler…"), true, None);
        let options = MenuItem::new(crate::i18n::t("Options…"), true, None);
        let about = MenuItem::new(crate::i18n::t("About"), true, None);
        let exit = MenuItem::new(crate::i18n::t("Exit"), true, None);
        menu.append(&show).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&add).ok()?;
        menu.append(&resume).ok()?;
        menu.append(&stop).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&limiter).ok()?;
        menu.append(&sched).ok()?;
        menu.append(&options).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&about).ok()?;
        menu.append(&exit).ok()?;
        let icon = Icon::from_rgba(include_bytes!("../../ui/tray32.rgba").to_vec(), 32, 32).ok()?;
        let tray = TrayIconBuilder::new()
            .with_id(MAIN_ID)
            .with_tooltip("Aknan Download")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            // the menu on a right click only; a double click opens the window
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        let ids: Vec<(MenuId, fn() -> TrayAction)> = vec![
            (show.id().clone(), || TrayAction::Show),
            (add.id().clone(), || TrayAction::AddUrl),
            (resume.id().clone(), || TrayAction::ResumeAll),
            (stop.id().clone(), || TrayAction::StopAll),
            (limiter.id().clone(), || TrayAction::ToggleLimiter),
            (sched.id().clone(), || TrayAction::Scheduler),
            (options.id().clone(), || TrayAction::Options),
            (about.id().clone(), || TrayAction::About),
            (exit.id().clone(), || TrayAction::Exit),
        ];
        Some(Tray {
            _icon: tray,
            show,
            add,
            resume,
            stop,
            limiter,
            sched,
            options,
            about,
            exit,
            ids,
        })
    }

    /// Update all tray menu labels after a language change.
    pub fn refresh_labels(&self) {
        self.show.set_text(crate::i18n::t("Restore Aknan Download"));
        self.add.set_text(crate::i18n::t("Add new download…"));
        self.resume.set_text(crate::i18n::t("Resume all downloads"));
        self.stop.set_text(crate::i18n::t("Stop all downloads"));
        self.limiter.set_text(crate::i18n::t("Speed Limiter"));
        self.sched.set_text(crate::i18n::t("Scheduler…"));
        self.options.set_text(crate::i18n::t("Options…"));
        self.about.set_text(crate::i18n::t("About"));
        self.exit.set_text(crate::i18n::t("Exit"));
    }

    /// Clicks since the last call (those on hidden downloads go to
    /// [`super::take_download_actions`]).
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut out = Vec::new();
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            let id = ev.id().0.clone();
            if let Some(dl) = id.strip_prefix(DL_PREFIX).and_then(|n| n.parse::<u64>().ok()) {
                // one click on a hidden download's icon: its window
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    push_action(dl, DownloadAction::Show);
                }
                continue;
            }
            // left click: the window opens / goes away; a double click:
            // it opens (the menu is on the right button)
            match ev {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => out.push(TrayAction::Toggle),
                TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => out.push(TrayAction::Show),
                _ => {}
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some((_, make)) = self.ids.iter().find(|(id, _)| *id == ev.id) {
                out.push(make());
                continue;
            }
            handle_menu_id(&ev.id);
        }
        out
    }

    /// Keep the check mark of "Speed Limiter" in step with the app.
    pub fn set_limiter(&self, on: bool) {
        if self.limiter.is_checked() != on {
            self.limiter.set_checked(on);
        }
    }
}
