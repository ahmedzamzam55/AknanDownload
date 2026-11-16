/// Physical key -> ASCII shortcut character fallback when the active keyboard
/// layout is non-Latin (Arabic, Russian, etc.). Kept platform-neutral for unit
/// tests while `hook_window` handles live Windows Winit events.
#[cfg_attr(not(windows), allow(dead_code))]
fn physical_shortcut_char(code: slint::winit_030::winit::keyboard::KeyCode) -> Option<char> {
    use slint::winit_030::winit::keyboard::KeyCode;
    Some(match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::Comma => ',',
        _ => return None,
    })
}

/// Normalized shortcut action dispatched into Slint.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranslatedShortcut {
    /// `Ctrl + <ascii>` keeping the physical Shift state intact.
    CtrlChar(char),
    /// Synthesized `Ctrl + <ascii>` with Shift cleared (e.g. `Ctrl+Insert`, `Shift+Insert`).
    ForceCtrlNoShift(char),
}

/// Decides whether a key event should be translated to a canonical lowercase
/// ASCII shortcut for Slint (`Caps Lock`, Arabic/non-Latin layouts, `Insert` chords).
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn translate_shortcut(
    code: Option<slint::winit_030::winit::keyboard::KeyCode>,
    insert_named: bool,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    unmodified_text: Option<&str>,
    logical_text: Option<&str>,
) -> Option<TranslatedShortcut> {
    use slint::winit_030::winit::keyboard::KeyCode;
    if alt || meta {
        return None;
    }
    let is_insert = insert_named || code == Some(KeyCode::Insert);
    if is_insert {
        if ctrl && !shift {
            return Some(TranslatedShortcut::ForceCtrlNoShift('c'));
        }
        if !ctrl && shift {
            return Some(TranslatedShortcut::ForceCtrlNoShift('v'));
        }
        return None;
    }
    if !ctrl {
        return None;
    }
    let from_unmod = unmodified_text.and_then(|s| {
        let mut chars = s.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        if ch.is_ascii_alphabetic() || ch == ',' {
            Some(ch.to_ascii_lowercase())
        } else {
            None
        }
    });
    let target = from_unmod.or_else(|| code.and_then(physical_shortcut_char))?;
    let mut buf = [0u8; 4];
    let exact = target.encode_utf8(&mut buf);
    if logical_text == Some(exact) {
        return None;
    }
    Some(TranslatedShortcut::CtrlChar(target))
}

/// No-op compatibility entry point; all shortcut translation now lives inside
/// `hook_window` so `keep_drawn` never overwrites it.
#[allow(dead_code)]
pub(crate) fn latin_shortcuts(_w: &slint::Window) {}

/// Is the (main, left) mouse button held down right now? Windows itself is
/// asked, not the messages the program got.
#[cfg(windows)]
pub(crate) fn left_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // SAFETY: plain queries of the system state
    unsafe {
        // buttons swapped for left-handed use: the main one is the right one
        let main = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        (GetAsyncKeyState(main as i32) as u16 & 0x8000) != 0
    }
}

#[cfg(not(windows))]
pub(crate) fn left_button_down() -> bool {
    true
}

/// Are Ctrl and Shift held down right now (Windows asked)?
#[cfg(windows)]
pub(crate) fn ctrl_shift_held() -> Option<(bool, bool)> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
    // SAFETY: plain queries of the system state
    unsafe {
        let held = |k: u16| (GetAsyncKeyState(k as i32) as u16 & 0x8000) != 0;
        Some((held(VK_CONTROL), held(VK_SHIFT)))
    }
}

#[cfg(not(windows))]
pub(crate) fn ctrl_shift_held() -> Option<(bool, bool)> {
    None
}
