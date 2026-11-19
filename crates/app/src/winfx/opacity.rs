use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    // Per-HWND cache so repeating the same opacity never churns WS_EX_LAYERED.
    static APPLIED: RefCell<HashMap<isize, u32>> = RefCell::new(HashMap::new());
}

/// The Win32 handle of a shown window (for the native menu).
#[cfg(windows)]
pub(crate) fn hwnd(w: &slint::Window) -> Option<isize> {
    hwnd_of(w)
}

/// The Win32 handle of a shown window.
#[cfg(windows)]
pub(super) fn hwnd_of(w: &slint::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = w.window_handle();
    let wh = handle.window_handle().ok()?;
    match wh.as_raw() {
        RawWindowHandle::Win32(raw) => Some(raw.hwnd.get()),
        _ => None,
    }
}

/// Make `w` `pct` % solid (50..=100). Called again every second for the main
/// window: it does nothing when nothing changed.
pub(crate) fn set_opacity(w: &slint::Window, pct: u32) {
    let pct = pct.clamp(50, 100);
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{RedrawWindow, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, LWA_ALPHA,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED,
        };
        let Some(id) = hwnd_of(w) else { return };
        let hwnd = id as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let layered = ex & WS_EX_LAYERED as isize != 0;
            let cached = APPLIED.with(|a| a.borrow().get(&id).copied());
            if cached == Some(pct) && layered == (pct < 100) {
                return;
            }
            let style_changed = layered != (pct < 100);
            if pct >= 100 {
                if layered {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
                }
            } else {
                if !layered {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
                }
                SetLayeredWindowAttributes(hwnd, 0, (pct * 255 / 100) as u8, LWA_ALPHA);
            }
            if style_changed {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
            RedrawWindow(hwnd, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN);
        }
        APPLIED.with(|a| {
            a.borrow_mut().insert(id, pct);
        });
    }
    #[cfg(not(windows))]
    {
        let _ = w;
        APPLIED.with(|a| {
            a.borrow_mut().insert(0, pct);
        });
    }
}
