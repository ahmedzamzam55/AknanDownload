//! "Make Aknan Download the main program for torrents" (Options > Torrents):
//! .torrent files and magnet: links of Windows open in Aknan Download, or
//! no more. Written directly via Win32 Registry APIs in HKCU (no admin, no CLI tools).

/// On: Windows opens .torrent files and magnet: links with Aknan Download.
/// Off: only what is Aknan Download's own goes (another program's choice
/// stays as it is).
pub(crate) fn torrents(on: bool) {
    #[cfg(windows)]
    {
        use crate::winfx::registry::{delete_tree, delete_value, get_sz, set_sz};
        let Ok(exe) = std::env::current_exe() else { return };
        let exe = exe.display().to_string();
        let open = format!("\"{exe}\" \"%1\"");
        let icon = format!("{exe},0");
        if on {
            for (key, name, data) in [
                (r"Software\Classes\.torrent\OpenWithProgids", Some("AknanDownload.torrent"), ""),
                (r"Software\Classes\.torrent", None, "AknanDownload.torrent"),
                (r"Software\Classes\AknanDownload.torrent", None, "Torrent file"),
                (r"Software\Classes\AknanDownload.torrent\DefaultIcon", None, icon.as_str()),
                (r"Software\Classes\AknanDownload.torrent\shell\open\command", None, open.as_str()),
                (r"Software\Classes\magnet", None, "URL:Magnet link"),
                (r"Software\Classes\magnet", Some("URL Protocol"), ""),
                (r"Software\Classes\magnet\DefaultIcon", None, icon.as_str()),
                (r"Software\Classes\magnet\shell\open\command", None, open.as_str()),
            ] {
                set_sz(key, name, data);
            }
        } else {
            if get_sz(r"Software\Classes\.torrent", None).as_deref() == Some("AknanDownload.torrent") {
                delete_value(r"Software\Classes\.torrent", None);
            }
            delete_value(r"Software\Classes\.torrent\OpenWithProgids", Some("AknanDownload.torrent"));
            delete_tree(r"Software\Classes\AknanDownload.torrent");
            if get_sz(r"Software\Classes\magnet\shell\open\command", None)
                .is_some_and(|v| v.to_ascii_lowercase().contains("aknandownload.exe"))
            {
                delete_tree(r"Software\Classes\magnet");
            }
        }
        unsafe {
            use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
            SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
        }
    }
    #[cfg(not(windows))]
    let _ = on;
}
