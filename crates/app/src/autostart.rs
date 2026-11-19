//! "Start with Windows": a value under HKCU\...\Run that starts the app in
//! the background (next to the clock) when the user signs in, managed natively
//! via Win32 Registry APIs.

#[cfg(windows)]
mod imp {
    use crate::winfx::registry::{delete_value, get_sz, set_sz, value_exists};

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APP_KEY: &str = r"Software\Aknan\AknanDownload";
    const NAME: &str = "AknanDownload";

    pub fn is_enabled() -> bool {
        value_exists(KEY, Some(NAME))
    }

    /// Installed with the setup program (which asked about autostart itself).
    pub fn installed_by_setup() -> bool {
        value_exists(APP_KEY, Some("Installed"))
    }

    /// What the setup program wrote for the first start ("Language", "Torrents").
    pub fn setup_value(name: &str) -> Option<String> {
        get_sz(APP_KEY, Some(name)).filter(|v| !v.is_empty())
    }

    pub fn set(enabled: bool) -> bool {
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return false };
            let value = format!("\"{}\" --background", exe.display());
            set_sz(KEY, Some(NAME), &value)
        } else {
            !is_enabled() || delete_value(KEY, Some(NAME))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_enabled() -> bool { false }
    pub fn set(_enabled: bool) -> bool { false }
    pub fn installed_by_setup() -> bool { false }
    pub fn setup_value(_name: &str) -> Option<String> { None }
}

pub use imp::{installed_by_setup, is_enabled, set, setup_value};
