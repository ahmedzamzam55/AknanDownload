use super::DownloadAction;
use std::collections::HashMap;
use tray_icon::menu::{Menu, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub(super) const DL_PREFIX: &str = "aknan-dl-";

/// The icon of one hidden download window.
struct Mini {
    icon: TrayIcon,
    show: MenuId,
    pause: MenuId,
    /// what it shows now, to change it only when needed
    step: Option<u32>,
    tip: String,
}

thread_local! {
    static HIDDEN: std::cell::RefCell<HashMap<u64, Mini>> = std::cell::RefCell::new(HashMap::new());
    static PENDING: std::cell::RefCell<Vec<(u64, DownloadAction)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn icon_for(progress: Option<f32>) -> Option<Icon> {
    Icon::from_rgba(super::download_icon(progress), 32, 32).ok()
}

/// A small icon next to the clock for a download whose window was hidden.
pub fn show_download(id: u64, tip: &str, progress: Option<f32>, labels: (&str, &str)) -> bool {
    if HIDDEN.with(|h| h.borrow().contains_key(&id)) {
        return true;
    }
    let menu = Menu::new();
    let show = MenuItem::new(labels.0, true, None);
    let pause = MenuItem::new(labels.1, true, None);
    if menu.append(&show).is_err() || menu.append(&pause).is_err() {
        return false;
    }
    let Some(icon) = icon_for(progress) else { return false };
    let built = TrayIconBuilder::new()
        .with_id(format!("{DL_PREFIX}{id}"))
        .with_tooltip(super::short_tip(tip))
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build();
    let Ok(icon) = built else { return false };
    let mini = Mini {
        icon,
        show: show.id().clone(),
        pause: pause.id().clone(),
        step: progress.map(super::step_of),
        tip: tip.to_string(),
    };
    HIDDEN.with(|h| h.borrow_mut().insert(id, mini));
    true
}

/// New numbers for its icon and its hint.
pub fn update_download(id: u64, tip: &str, progress: Option<f32>) {
    HIDDEN.with(|h| {
        if let Some(m) = h.borrow_mut().get_mut(&id) {
            let step = progress.map(super::step_of);
            if step != m.step {
                m.step = step;
                let _ = m.icon.set_icon(icon_for(progress));
            }
            if m.tip != tip {
                m.tip = tip.to_string();
                let _ = m.icon.set_tooltip(Some(super::short_tip(tip)));
            }
        }
    });
}

/// Its window is back (or it is done): the icon goes.
pub fn remove_download(id: u64) {
    HIDDEN.with(|h| h.borrow_mut().remove(&id));
}

/// Clicks on the icons of hidden downloads since the last call.
pub fn take_download_actions() -> Vec<(u64, DownloadAction)> {
    PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

pub(super) fn push_action(dl: u64, action: DownloadAction) {
    PENDING.with(|p| p.borrow_mut().push((dl, action)));
}

pub(super) fn handle_menu_id(ev_id: &MenuId) {
    let hit = HIDDEN.with(|h| {
        h.borrow().iter().find_map(|(dl, m)| {
            if *ev_id == m.show {
                Some((*dl, DownloadAction::Show))
            } else if *ev_id == m.pause {
                Some((*dl, DownloadAction::Pause))
            } else {
                None
            }
        })
    });
    if let Some(hit) = hit {
        push_action(hit.0, hit.1);
    }
}
