use std::io::Write;
use swiftget_engine::default_data_dir;

/// One line in a small log next to the downloads list (data\events.log):
/// when the program started, closed, updated itself, could not start...
/// what someone asking for help can send.
pub(crate) fn note(what: &str) {
    let dir = default_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("events.log");
    if std::fs::metadata(&path).map(|m| m.len() > 128 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    let line = format!("{} {} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), what);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// A crash leaves a note (what and where, no personal data) in crash.log in
/// the program's data folder, so it can be found and fixed.
pub(super) fn install_crash_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = default_data_dir().join("crash.log");
        // keep the file small: start again when it gets big
        if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
            let _ = std::fs::remove_file(&path);
        }
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let line = format!(
            "{} version {} thread {:?}: {} at {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            env!("CARGO_PKG_VERSION"),
            std::thread::current().name().unwrap_or("?"),
            what,
            place
        );
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line.as_bytes());
        }
        previous(info);
    }));
}

/// Opened "as administrator" (for the log: the program never needs it).
pub(super) fn elevated() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; it only reads the token of this process
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    false
}

/// End a copy of this program that hangs (only ever one of ours: the id
/// came from its own answer on our port, and the name is checked too).
pub(super) fn end_process(pid: u32) {
    #[cfg(windows)]
    {
        use std::path::Path;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_TERMINATE,
        };
        let Some(my_name) = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_string())) else {
            return;
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return;
            }
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                if Path::new(&full).file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&my_name)) {
                    let _ = TerminateProcess(h, 1);
                }
            }
            let _ = CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}
