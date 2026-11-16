/// A file / folder picker that belongs to `w`: it stays on top of it and
/// `w` waits for it.
#[cfg(windows)]
pub(crate) fn on_top_of(dlg: rfd::FileDialog, w: &slint::Window) -> rfd::FileDialog {
    dlg.set_parent(&w.window_handle())
}

#[cfg(not(windows))]
pub(crate) fn on_top_of(dlg: rfd::FileDialog, _w: &slint::Window) -> rfd::FileDialog {
    dlg
}

/// The window of ours the user is working in (in front), if any.
#[cfg(windows)]
fn own_front_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries about the window in front; `pid` is a local
    let (hwnd, pid) = unsafe {
        let h = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        (h, pid)
    };
    (!hwnd.is_null() && pid == std::process::id()).then_some(hwnd as isize)
}

#[cfg(windows)]
struct OwnWindow(isize);

#[cfg(windows)]
impl raw_window_handle::HasWindowHandle for OwnWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{HandleError, RawWindowHandle, Win32WindowHandle, WindowHandle};
        let raw = Win32WindowHandle::new(std::num::NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?);
        // SAFETY: the handle is of a window of ours that is open now; the
        // message box using it is shown and closed before we go on
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) })
    }
}

#[cfg(windows)]
impl raw_window_handle::HasDisplayHandle for OwnWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
    }
}

/// A message box that belongs to the window in front: it opens on top of
/// it (never hidden behind, which looked like the program had frozen).
pub(crate) fn msg_box() -> rfd::MessageDialog {
    let d = rfd::MessageDialog::new();
    #[cfg(windows)]
    if let Some(h) = own_front_window() {
        return d.set_parent(&OwnWindow(h));
    }
    d
}

/// The same for file / folder pickers.
pub(crate) fn file_box() -> rfd::FileDialog {
    // the windows wait for the picker: no "does not answer" meanwhile (the
    // main timer clears this once the windows move again)
    crate::api::UI_WAITS.store(true, std::sync::atomic::Ordering::Relaxed);
    let d = rfd::FileDialog::new();
    #[cfg(windows)]
    if let Some(h) = own_front_window() {
        return d.set_parent(&OwnWindow(h));
    }
    d
}

/// True when one of Aknan Download's own windows is currently the OS foreground window.
pub(crate) fn is_own_window_front() -> bool {
    #[cfg(windows)]
    {
        own_front_window().is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
