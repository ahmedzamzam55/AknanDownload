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
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_string()));
        let exe = exe.unwrap_or_else(|| "AknanDownload.exe".into());
        let _ = std::process::Command::new(swiftget_engine::system::windows_tool("taskkill.exe"))
            .args(["/F", "/PID", &pid.to_string(), "/FI", &format!("IMAGENAME eq {exe}")])
            .creation_flags(0x0800_0000) // no console window
            .status();
    }
    #[cfg(not(windows))]
    let _ = pid;
}
