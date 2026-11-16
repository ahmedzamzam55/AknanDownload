use super::shortcuts::{translate_shortcut, TranslatedShortcut};
use slint::platform::{Key as SlintKey, WindowEvent as SlintWindowEvent};
use slint::winit_030::winit::event::{ElementState, KeyEvent};
use slint::winit_030::winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use slint::winit_030::winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use slint::winit_030::EventResult;
use std::cell::Cell;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

const B_LC: u8 = 1 << 0;
const B_RC: u8 = 1 << 1;
const B_LS: u8 = 1 << 2;
const B_RS: u8 = 1 << 3;
const B_LA: u8 = 1 << 4;
const B_RA: u8 = 1 << 5;
const B_LM: u8 = 1 << 6;
const B_RM: u8 = 1 << 7;

#[derive(Clone, Copy, Default)]
pub(super) struct OsMods(pub(super) u8);

impl OsMods {
    pub(super) fn poll() -> Self {
        let held = |vk: u16| -> bool {
            // SAFETY: plain read of the OS async key state.
            unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
        };
        let rc = held(VK_RCONTROL);
        let lc = held(VK_LCONTROL) || (!rc && held(VK_CONTROL));
        let rs = held(VK_RSHIFT);
        let ls = held(VK_LSHIFT) || (!rs && held(VK_SHIFT));
        let ra = held(VK_RMENU);
        let la = held(VK_LMENU) || (!ra && held(VK_MENU));
        let lm = held(VK_LWIN);
        let rm = held(VK_RWIN);
        let mut m = 0u8;
        if lc { m |= B_LC; }
        if rc { m |= B_RC; }
        if ls { m |= B_LS; }
        if rs { m |= B_RS; }
        if la { m |= B_LA; }
        if ra { m |= B_RA; }
        if lm { m |= B_LM; }
        if rm { m |= B_RM; }
        Self(m)
    }
    pub(super) fn ctrl(self) -> bool { self.0 & (B_LC | B_RC) != 0 }
    pub(super) fn shift(self) -> bool { self.0 & (B_LS | B_RS) != 0 }
    pub(super) fn alt(self) -> bool { self.0 & (B_LA | B_RA) != 0 }
    pub(super) fn meta(self) -> bool { self.0 & (B_LM | B_RM) != 0 }
}

pub(super) fn modifier_bit(code: KeyCode) -> Option<u8> {
    Some(match code {
        KeyCode::ControlLeft => B_LC,
        KeyCode::ControlRight => B_RC,
        KeyCode::ShiftLeft => B_LS,
        KeyCode::ShiftRight => B_RS,
        KeyCode::AltLeft => B_LA,
        KeyCode::AltRight => B_RA,
        KeyCode::SuperLeft => B_LM,
        KeyCode::SuperRight => B_RM,
        _ => return None,
    })
}

const SLINT_KEYS: [(u8, SlintKey); 8] = [
    (B_LC, SlintKey::Control),
    (B_RC, SlintKey::ControlR),
    (B_LS, SlintKey::Shift),
    (B_RS, SlintKey::ShiftR),
    (B_LA, SlintKey::Alt),
    (B_RA, SlintKey::AltGr),
    (B_LM, SlintKey::Meta),
    (B_RM, SlintKey::MetaR),
];

pub(super) fn release_all(win: &slint::Window, tracked: &Cell<u8>) {
    for (_, k) in SLINT_KEYS {
        let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyReleased { text: k.into() });
    }
    tracked.set(0);
}

pub(super) fn release_stale_alt(win: &slint::Window, tracked: &Cell<u8>) {
    if tracked.get() & (B_LA | B_RA) != 0 {
        let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyReleased { text: SlintKey::Alt.into() });
        let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyReleased { text: SlintKey::AltGr.into() });
        tracked.set(tracked.get() & !(B_LA | B_RA));
    }
}

pub(super) fn sync_from_os(win: &slint::Window, tracked: &Cell<u8>, os: OsMods, force_alt_clear: bool) {
    let prev = tracked.get();
    for (bit, k) in SLINT_KEYS {
        let want = os.0 & bit != 0;
        let had = prev & bit != 0;
        if !want && (had || (force_alt_clear && matches!(bit, B_LA | B_RA))) {
            let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyReleased { text: k.into() });
        } else if want && !had {
            let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyPressed { text: k.into() });
        }
    }
    tracked.set(os.0);
}

fn set_ctrl_no_shift(win: &slint::Window, tracked: &Cell<u8>) {
    for (bit, k) in [
        (B_LS, SlintKey::Shift),
        (B_RS, SlintKey::ShiftR),
        (B_LA, SlintKey::Alt),
        (B_RA, SlintKey::AltGr),
        (B_LM, SlintKey::Meta),
        (B_RM, SlintKey::MetaR),
    ] {
        if tracked.get() & bit != 0 {
            let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyReleased { text: k.into() });
            tracked.set(tracked.get() & !bit);
        }
    }
    if tracked.get() & (B_LC | B_RC) == 0 {
        let _ = win.dispatch_event_with_result(SlintWindowEvent::KeyPressed { text: SlintKey::Control.into() });
        tracked.set(tracked.get() | B_LC);
    }
}

fn dispatch_translated_key(
    win: &slint::Window,
    tracked: &Cell<u8>,
    os: OsMods,
    trans: TranslatedShortcut,
    pressed: bool,
    repeat: bool,
) {
    let ch = match trans {
        TranslatedShortcut::CtrlChar(c) => {
            sync_from_os(win, tracked, os, !os.alt());
            c
        }
        TranslatedShortcut::ForceCtrlNoShift(c) => {
            set_ctrl_no_shift(win, tracked);
            c
        }
    };
    let text: slint::SharedString = ch.into();
    let slint_ev = if !pressed {
        SlintWindowEvent::KeyReleased { text }
    } else if repeat {
        SlintWindowEvent::KeyPressRepeated { text }
    } else {
        SlintWindowEvent::KeyPressed { text }
    };
    let _ = win.dispatch_event_with_result(slint_ev);
    if matches!(trans, TranslatedShortcut::ForceCtrlNoShift(_)) {
        sync_from_os(win, tracked, os, false);
    }
}

pub(super) fn handle_keyboard_input(
    win: &slint::Window,
    event: &KeyEvent,
    tracked_mods: &Cell<u8>,
    active_translated: &Cell<Option<(KeyCode, TranslatedShortcut)>>,
) -> Option<EventResult> {
    let os = OsMods::poll();
    let pressed = event.state == ElementState::Pressed;
    if let PhysicalKey::Code(code) = event.physical_key {
        if let Some(bit) = modifier_bit(code) {
            if !os.alt() {
                release_stale_alt(win, tracked_mods);
            }
            let mut m = tracked_mods.get();
            if pressed { m |= bit; } else { m &= !bit; }
            tracked_mods.set(m);
            return Some(EventResult::Propagate);
        }
        if !pressed {
            if let Some((prev_code, prev_trans)) = active_translated.get() {
                if prev_code == code {
                    active_translated.set(None);
                    dispatch_translated_key(win, tracked_mods, os, prev_trans, false, false);
                    return Some(EventResult::PreventDefault);
                }
            }
        }
    }
    let code = match event.physical_key {
        PhysicalKey::Code(c) => Some(c),
        PhysicalKey::Unidentified(_) => None,
    };
    let insert_named = matches!(event.logical_key, Key::Named(NamedKey::Insert));
    let unmod = event.key_without_modifiers();
    let unmod_text = match &unmod {
        Key::Character(s) => Some(s.as_str()),
        _ => None,
    };
    let logical_text = match &event.logical_key {
        Key::Character(s) => Some(s.as_str()),
        _ => None,
    };
    if let Some(trans) = translate_shortcut(code, insert_named, os.ctrl(), os.shift(), os.alt(), os.meta(), unmod_text, logical_text) {
        if let Some(c) = code {
            active_translated.set(if pressed { Some((c, trans)) } else { None });
        }
        dispatch_translated_key(win, tracked_mods, os, trans, pressed, event.repeat);
        return Some(EventResult::PreventDefault);
    }
    sync_from_os(win, tracked_mods, os, !os.alt() && os.ctrl());
    None
}
