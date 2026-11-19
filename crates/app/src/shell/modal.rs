use super::look::{set_theme_of, Look};
use super::state::{Ctx, APP};
use crate::{api, winfx, ConfirmDialog, Theme};
use slint::{ComponentHandle, Timer};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    /// a Windows message box is open: its own message loop runs our timers,
    /// and a list refresh in the middle of the click that opened it could
    /// hang the program; the timers wait until it is closed
    static MODAL: Cell<bool> = const { Cell::new(false) };
    // the question windows that are open now
    static CONFIRMS: RefCell<Vec<ConfirmDialog>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn modal_open() -> bool {
    MODAL.with(|m| m.get())
}

pub(crate) fn suspend_modals() -> ModalGuard {
    ModalGuard::new()
}

/// Marks a message box as open for as long as it lives.
pub(crate) struct ModalGuard(bool);

impl ModalGuard {
    pub(crate) fn new() -> ModalGuard {
        api::UI_WAITS.store(true, std::sync::atomic::Ordering::Relaxed);
        ModalGuard(MODAL.with(|m| m.replace(true)))
    }
}

impl Drop for ModalGuard {
    fn drop(&mut self) {
        let before = self.0;
        MODAL.with(|m| m.set(before));
        api::UI_WAITS.store(before, std::sync::atomic::Ordering::Relaxed);
        // waiting for the box was no hang
        api::UI_ALIVE.store(api::now_secs(), std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn message(text: &str) {
    let _g = ModalGuard::new();
    winfx::msg_box().set_title(APP).set_description(breakable(text)).show();
}

/// A text that can be wrapped anywhere it is long: words longer than 32
/// letters (addresses, paths, error codes) get places to break (an invisible
/// space), so a window with it never gets wider than the screen.
pub(crate) fn breakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut run = 0;
    for c in text.chars() {
        if c.is_whitespace() {
            run = 0;
        } else {
            run += 1;
            if run > 32 {
                out.push('\u{200B}');
                run = 1;
            }
        }
        out.push(c);
    }
    out
}

pub(crate) fn ask(text: &str) -> bool {
    use rfd::{MessageButtons, MessageDialogResult};
    let _g = ModalGuard::new();
    matches!(
        winfx::msg_box().set_title(APP).set_description(text).set_buttons(MessageButtons::YesNo).show(),
        MessageDialogResult::Yes
    )
}

/// What runs when a question is answered with OK.
type OnOk = Box<dyn FnOnce(bool, bool)>;

/// A yes / no question in a window of our own: it is shown in front of the
/// download windows and the program goes on working while it waits (a
/// Windows message box could end up behind them and seem to freeze it all).
/// `on_ok` runs when OK is pressed.
pub(crate) fn confirm(ctx: &Ctx, title: &str, text: &str, on_ok: impl FnOnce() + 'static) {
    confirm_with(ctx, title, text, None, move |_, _| on_ok());
}

/// The same; `delete_files`: a question about removing downloads, with its
/// two check boxes (this one ticked as given). `on_ok` gets both of them.
pub(super) fn confirm_with(ctx: &Ctx, title: &str, text: &str, delete_files: Option<bool>, on_ok: impl FnOnce(bool, bool) + 'static) {
    let Ok(d) = ConfirmDialog::new() else {
        if ask(text) {
            on_ok(false, false);
        }
        return;
    };
    set_theme_of(d.global::<Theme>(), &Look::of(&ctx.mgr.settings()));
    d.set_heading(title.into());
    d.set_text(text.into());
    d.set_removing(delete_files.is_some());
    d.set_delete_files(delete_files.unwrap_or(false));
    let pending: Rc<RefCell<Option<OnOk>>> = Rc::new(RefCell::new(Some(Box::new(on_ok))));
    let close = |d: &ConfirmDialog| {
        let _ = d.hide();
        // dropped after this event is over
        Timer::single_shot(Duration::from_millis(1), || {
            CONFIRMS.with(|c| c.borrow_mut().retain(|x| x.window().is_visible()));
        });
    };
    {
        let (w, pending) = (d.as_weak(), pending.clone());
        d.on_answered(move |ok| {
            let Some(d) = w.upgrade() else { return };
            close(&d);
            let f = pending.borrow_mut().take();
            if ok {
                if let Some(f) = f {
                    f(d.get_delete_files(), d.get_dont_ask());
                }
            }
        });
    }
    {
        let w = d.as_weak();
        d.window().on_close_requested(move || {
            if let Some(d) = w.upgrade() {
                close(&d);
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    open_window!(d);
    CONFIRMS.with(|c| c.borrow_mut().push(d));
}
