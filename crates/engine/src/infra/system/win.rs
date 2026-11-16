use super::AfterDone;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW};

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn free_space(dir: &Path) -> Option<u64> {
    let w = wide(dir);
    let mut free: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

pub fn file_system(dir: &Path) -> Option<String> {
    let w = wide(dir);
    let mut root = [0u16; 300];
    if unsafe { GetVolumePathNameW(w.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return None;
    }
    let mut name = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..len]))
}

pub fn power(action: AfterDone, force: bool) {
    use std::os::windows::process::CommandExt;
    let run = |args: &[&str]| {
        let _ = std::process::Command::new(super::windows_tool("shutdown.exe"))
            .args(args)
            .creation_flags(0x0800_0000)
            .spawn();
    };
    match action {
        AfterDone::Shutdown => {
            if force {
                run(&["/s", "/f", "/t", "0"]);
            } else {
                run(&["/s", "/t", "0"]);
            }
        }
        AfterDone::Hibernate => run(&["/h"]),
        AfterDone::Sleep => unsafe {
            windows_sys::Win32::System::Power::SetSuspendState(0, u8::from(force), 0);
        },
        _ => {}
    }
}

pub fn idle_secs() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: a plain structure of the right size
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    let now = unsafe { GetTickCount() };
    Some(u64::from(now.wrapping_sub(info.dwTime)) / 1000)
}

/// A waitable timer that wakes the computer; closed when dropped.
pub struct WakeHandle(windows_sys::Win32::Foundation::HANDLE);
// SAFETY: a kernel handle, only closed once (Drop)
unsafe impl Send for WakeHandle {}
unsafe impl Sync for WakeHandle {}
impl Drop for WakeHandle {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

pub fn wake_at(at: chrono::DateTime<chrono::Utc>) -> Option<WakeHandle> {
    use windows_sys::Win32::System::Threading::{CreateWaitableTimerW, SetWaitableTimer};
    // 100 ns steps since 1601 (positive: a time of the clock, not a wait)
    let due: i64 = (at.timestamp() + 11_644_473_600) * 10_000_000;
    // SAFETY: no name, no security attributes; the handle is closed by WakeHandle
    let h = unsafe { CreateWaitableTimerW(std::ptr::null(), 1, std::ptr::null()) };
    if h.is_null() {
        return None;
    }
    let handle = WakeHandle(h);
    // the last 1: wake the computer when it comes
    let ok = unsafe { SetWaitableTimer(h, &due, 0, None, std::ptr::null(), 1) };
    (ok != 0).then_some(handle)
}
