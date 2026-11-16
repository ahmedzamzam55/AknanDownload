use std::sync::atomic::Ordering;
use std::time::Duration;
use swiftget_engine::sync::LockOk;

/// Addresses the user chose to download with the browser itself (the server
/// refused Aknan Download): the extension leaves them to the browser once.
static FOR_BROWSER: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());

/// Seconds (since 1970) when the window side of the program last moved; the
/// main timer sets it a few times a second. Far behind = the windows hang.
pub static UI_ALIVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// a message box / file picker of ours is open (the windows wait for it)
pub static UI_WAITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The random port the add-in talks to (0 = not open).
pub static MAIN_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
/// The known port is this program's (another program or Windows may keep it).
pub static FRONT_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// When an add-in last talked to the program with its key (seconds since 1970),
/// and when one last came through the browser's helper.
pub static ADDIN_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static HELPER_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Changed by "Repair the browser add-in": the add-in sees it and starts again.
pub static REPAIR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// The secret key of this program (Settings::api_token), set at start.
pub static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Version of the browser add-in that ships with this program.
pub fn bundled_extension_version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../../../../extension/manifest.json"))
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn leave_to_browser(url: &str) {
    let mut l = FOR_BROWSER.lock_ok();
    l.retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
    l.push((url.to_string(), std::time::Instant::now()));
}

pub(super) fn take_for_browser(urls: &[String]) -> bool {
    let mut l = FOR_BROWSER.lock_ok();
    l.retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
    if let Some(i) = l.iter().position(|(u, _)| urls.iter().any(|x| x == u)) {
        l.remove(i);
        return true;
    }
    false
}

/// What Windows may hand over to open: a magnet link or a .torrent file
/// that is there. Anything else is refused.
pub fn torrent_source_ok(s: &str) -> bool {
    let s = s.trim();
    swiftget_engine::torrent::is_magnet(s)
        || (s.to_ascii_lowercase().ends_with(".torrent") && std::path::Path::new(s).is_file())
}

/// What the extension needs to know from Options (file types to take,
/// sites to leave alone, keys, the video panel).
pub fn config_json(s: &swiftget_engine::Settings) -> String {
    let split = |t: &str| t.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>();
    serde_json::json!({
        "ok": true,
        "language": crate::i18n::effective(&s.language),
        "captureExts": split(&s.capture_exts.to_ascii_lowercase()),
        "skipSites": split(&s.skip_sites),
        "skipAddresses": s.skip_addresses,
        "skipKey": s.skip_key,
        "forceKey": s.force_key,
        "mp3Kbps": s.mp3_kbps,
        "torrents": s.torrent_main,
        "panel": {
            "enabled": s.panel_enabled,
            "skipSites": split(&s.panel_skip_sites),
            "minVideoKb": s.panel_min_video_kb,
            "minAudioKb": s.panel_min_audio_kb,
        },
    })
    .to_string()
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(super) fn ui_hangs() -> bool {
    let last = UI_ALIVE.load(Ordering::Relaxed);
    last != 0 && !UI_WAITS.load(Ordering::Relaxed) && now_secs().saturating_sub(last) > 30
}
