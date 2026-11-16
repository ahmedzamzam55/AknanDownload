use std::cell::Cell;

thread_local! {
    // (window handle, percent) applied last
    static APPLIED: Cell<(isize, u32)> = const { Cell::new((0, 100)) };
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
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        };
        let Some(id) = hwnd_of(w) else { return };
        let hwnd = id as windows_sys::Win32::Foundation::HWND;
        // SAFETY: hwnd is the live window of `w`; only its extended style and
        // its alpha are changed
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let layered = ex & WS_EX_LAYERED as isize != 0;
            // the window may have got its style back from the toolkit
            if APPLIED.with(|a| a.get()) == (id, pct) && layered == (pct < 100) {
                return;
            }
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
        }
        APPLIED.with(|a| a.set((id, pct)));
    }
    #[cfg(not(windows))]
    {
        let _ = w;
        APPLIED.with(|a| a.set((0, pct)));
    }
}
