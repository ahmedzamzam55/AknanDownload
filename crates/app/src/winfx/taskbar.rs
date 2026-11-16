#[cfg(windows)]
use super::opacity::hwnd_of;

/// What the taskbar button of a window shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TaskbarProgress {
    Off,
    /// size unknown: a moving bar
    Busy,
    Normal(u64, u64),
    Paused(u64, u64),
    Error(u64, u64),
}

#[cfg(windows)]
mod taskbar {
    use std::ffi::c_void;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

    // ITaskbarList3 (shobjidl.h), in the order of its methods
    #[repr(C)]
    struct Vtbl {
        _query_interface: usize,
        _add_ref: usize,
        _release: usize,
        hr_init: unsafe extern "system" fn(*mut c_void) -> i32,
        _add_tab: usize,
        _delete_tab: usize,
        _activate_tab: usize,
        _set_active_alt: usize,
        _mark_fullscreen_window: usize,
        set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> i32,
        set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, i32) -> i32,
    }

    const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
    const IID_ITASKBAR_LIST3: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);
    pub const NOPROGRESS: i32 = 0;
    pub const INDETERMINATE: i32 = 1;
    pub const NORMAL: i32 = 2;
    pub const ERROR: i32 = 4;
    pub const PAUSED: i32 = 8;

    thread_local! {
        // made once on the UI thread and kept for the life of the program
        static LIST: usize = unsafe {
            let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as _);
            let mut p: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ITASKBAR_LIST3, &mut p);
            if hr < 0 || p.is_null() {
                0
            } else {
                let vt = *(p as *const *const Vtbl);
                ((*vt).hr_init)(p);
                p as usize
            }
        };
    }

    pub fn set(hwnd: isize, state: i32, value: Option<(u64, u64)>) {
        LIST.with(|&p| {
            if p == 0 {
                return;
            }
            let p = p as *mut c_void;
            // SAFETY: p is the ITaskbarList3 made above (never released);
            // hwnd is a live window of this program
            unsafe {
                let vt = *(p as *const *const Vtbl);
                ((*vt).set_progress_state)(p, hwnd as HWND, state);
                if let Some((done, total)) = value {
                    ((*vt).set_progress_value)(p, hwnd as HWND, done, total.max(1));
                }
            }
        });
    }
}

/// Show `p` on the taskbar button of `w` (the window may be minimized).
pub(crate) fn taskbar_progress(w: &slint::Window, p: TaskbarProgress) {
    #[cfg(windows)]
    {
        use taskbar::*;
        let Some(hwnd) = hwnd_of(w) else { return };
        match p {
            TaskbarProgress::Off => set(hwnd, NOPROGRESS, None),
            TaskbarProgress::Busy => set(hwnd, INDETERMINATE, None),
            TaskbarProgress::Normal(d, t) => set(hwnd, NORMAL, Some((d, t))),
            TaskbarProgress::Paused(d, t) => set(hwnd, PAUSED, Some((d, t))),
            TaskbarProgress::Error(d, t) => set(hwnd, ERROR, Some((d, t))),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (w, p);
    }
}
