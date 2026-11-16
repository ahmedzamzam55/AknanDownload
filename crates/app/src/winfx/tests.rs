use super::placement::centered;
use super::shortcuts::{translate_shortcut, TranslatedShortcut};
use slint::winit_030::winit::keyboard::KeyCode;

#[test]
fn windows_open_in_the_middle() {
    assert_eq!(centered((0, 0), (1920, 1080), (800, 600)), (560, 240));
    // a second screen to the right
    assert_eq!(centered((1920, 0), (1280, 1024), (640, 480)), (2240, 272));
    // bigger than the screen: its top left corner stays on the screen
    assert_eq!(centered((0, 0), (1366, 768), (1600, 900)), (0, 0));
}

#[test]
fn shortcuts_normalize_arabic_caps_and_insert() {
    // Arabic layout Ctrl+C ('ؤ') / Ctrl+V ('ر') / Ctrl+A ('ش')
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyC), false, true, false, false, false, Some("ؤ"), Some("ؤ")),
        Some(TranslatedShortcut::CtrlChar('c'))
    );
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyV), false, true, false, false, false, Some("ر"), Some("ر")),
        Some(TranslatedShortcut::CtrlChar('v'))
    );
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyA), false, true, false, false, false, Some("ش"), Some("ش")),
        Some(TranslatedShortcut::CtrlChar('a'))
    );
    // English Caps Lock ON: logical/unmodified text is uppercase ASCII
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyC), false, true, false, false, false, Some("C"), Some("C")),
        Some(TranslatedShortcut::CtrlChar('c'))
    );
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyV), false, true, false, false, false, Some("V"), Some("V")),
        Some(TranslatedShortcut::CtrlChar('v'))
    );
    // English lowercase needs no override when logical text already matches
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyC), false, true, false, false, false, Some("c"), Some("c")),
        None
    );
    // Control characters (e.g., Ctrl+C -> \u{3}) still normalize
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyC), false, true, false, false, false, Some("c"), Some("\u{3}")),
        Some(TranslatedShortcut::CtrlChar('c'))
    );
    // Ctrl+Shift+Z on Arabic layout -> Ctrl+Shift+'z'
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyZ), false, true, true, false, false, Some("ئ"), Some("ئ")),
        Some(TranslatedShortcut::CtrlChar('z'))
    );
    // Ctrl+Insert & Shift+Insert
    assert_eq!(
        translate_shortcut(Some(KeyCode::Insert), true, true, false, false, false, None, None),
        Some(TranslatedShortcut::ForceCtrlNoShift('c'))
    );
    assert_eq!(
        translate_shortcut(Some(KeyCode::Insert), true, false, true, false, false, None, None),
        Some(TranslatedShortcut::ForceCtrlNoShift('v'))
    );
    // Alt / AltGr or Win key combinations are untouched
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyC), false, true, false, true, false, Some("ؤ"), Some("ؤ")),
        None
    );
    assert_eq!(
        translate_shortcut(Some(KeyCode::KeyV), false, false, false, false, true, Some("v"), Some("v")),
        None
    );
}
