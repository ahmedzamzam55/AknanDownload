use super::{CategoryDef, QueueDef, Schedule, ServerLimit, SiteLogin};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// missing in old files -> 0, so they get migrated
    #[serde(default)]
    pub version: u32,
    pub download_dir: PathBuf,
    /// Save into Downloads\Video, Downloads\Music ...
    pub category_folders: bool,
    /// Connections per download
    pub connections: u32,
    /// Downloads running at the same time (0 = no limit)
    pub max_active: u32,
    /// KB/s, 0 = unlimited
    pub speed_limit_kb: u64,
    pub retries: u32,
    pub user_agent: String,
    pub clipboard_monitor: bool,
    pub show_complete_dialog: bool,
    /// downloads added together (a playlist, a batch): the "Download
    /// complete" window for each file too, not only one at the end
    #[serde(default)]
    pub complete_each_in_group: bool,
    /// Removing downloads from the list asks first (unticked in that question:
    /// "Don't show this again"; Options > Downloads turns it back on)
    pub ask_before_remove: bool,
    /// ...and "Delete the downloaded files from the disk too" was ticked last time
    pub remove_deletes_files: bool,
    /// "Audio · MP3": the quality of the MP3 files made (kbps)
    pub mp3_kbps: u32,
    /// videos of sites (YouTube…): their subtitles too, in these languages
    pub subtitles: bool,
    pub subtitle_langs: String,
    pub browser_port: u16,
    /// A connection only splits a part when both halves are at least this big
    pub min_split_kb: u64,
    pub schedule: Schedule,
    /// progress window for each download
    pub show_progress_window: bool,
    pub queues: Vec<QueueDef>,
    /// 0 Light, 1 Dark, 2 Ocean, 3 Midnight, 4 Forest, 5 Black, 6 Sunset,
    /// 7 Teal night, 8 Rose
    pub theme: u32,
    /// how solid the main window is, 50..=100 (%)
    pub opacity: u32,
    /// size of all texts, 80..=150 (%)
    pub font_scale: u32,
    /// font of all windows ("" = Windows' own, Segoe UI)
    pub font_family: String,
    /// a picture behind the main window ("" = none, the theme's colors)
    pub background_image: String,
    /// how much of the picture shows through the panels, 10..=80 (%)
    pub background_strength: u32,
    /// version that ran last time: after an update "What's new" is shown once
    pub last_version: String,
    /// speed limiter switched on (the limit itself is speed_limit_kb)
    pub speed_limit_on: bool,
    /// the speed limit only while someone uses the computer: a minute
    /// without the mouse or the keyboard, full speed
    #[serde(default)]
    pub limit_only_active: bool,
    /// torrents: minutes of sharing after one is complete (0 = stop at once)
    #[serde(default)]
    pub torrent_seed_minutes: u32,
    /// torrents: most upload speed in KB/s (0 = no limit)
    #[serde(default)]
    pub torrent_upload_kb: u64,
    /// torrents: stop sharing once this much was sent for what was received
    /// (0 = no such limit; the minutes above still count)
    #[serde(default)]
    pub torrent_seed_ratio: f32,
    /// Aknan Download is the main program for torrents: magnet links and
    /// .torrent files from the browser (the add-in) and from Windows open in
    /// it. Off: they stay with the browser / the torrent program of the user
    pub torrent_main: bool,
    /// "remember this path for this category" from the download dialog
    pub category_dirs: std::collections::BTreeMap<String, PathBuf>,
    pub hide_categories: bool,
    /// list sorting: "name", "size", "status", "date"; and direction
    pub sort_by: String,
    pub sort_desc: bool,
    /// closing the window keeps the app running next to the clock
    pub close_to_tray: bool,
    /// "start with Windows" was set up once (first run)
    pub autostart_set: bool,

    // ---- v4: everything of the classic Options window
    /// sidebar categories (General is implicit, always last)
    pub categories: Vec<CategoryDef>,
    /// start downloading links copied to the clipboard without asking
    pub auto_start_clipboard: bool,
    /// double click on a download: 0 open file / progress, 1 properties
    pub double_click: u32,
    /// speed limiter is switched on every time the app starts
    pub limiter_on_startup: bool,
    /// "Tip of the day" at startup
    pub show_tips: bool,
    pub tip_index: u32,
    /// browser: extensions handed to us, space separated ("R0*" style patterns)
    pub capture_exts: String,
    /// browser: never take downloads from these sites (space separated, * allowed)
    pub skip_sites: String,
    /// browser: never take these addresses (* allowed)
    pub skip_addresses: Vec<String>,
    /// browser: key that lets the browser download by itself ("alt", "ctrl", "shift", "none")
    pub skip_key: String,
    /// browser: key that forces Aknan Download to take any link
    pub force_key: String,
    /// "Download this video" panel on web players
    pub panel_enabled: bool,
    pub panel_skip_sites: String,
    pub panel_min_video_kb: u64,
    pub panel_min_audio_kb: u64,
    /// .sgpart files go here while downloading (None = next to the file)
    pub temp_dir: Option<PathBuf>,
    /// "Download File Info" window for downloads from the browser / clipboard
    pub show_start_dialog: bool,
    /// progress window: 0 normal, 1 minimized, 2 don't show
    pub progress_mode: u32,
    /// address already in the list: 0 ask, 1 add with a number, 2 replace the old one, 3 show the old one
    pub duplicate_action: u32,
    /// set the file date to the server's "Last-Modified"
    pub set_file_date: bool,
    /// scan every finished file with this program ("[File]" = the file)
    pub virus_scan: bool,
    pub virus_program: String,
    pub virus_args: String,
    /// Options > Connection type: 0 low, 1 medium, 2 high speed
    pub connection_type: u32,
    pub server_limits: Vec<ServerLimit>,
    /// download no more than quota_mb every quota_hours
    pub quota_on: bool,
    pub quota_mb: u64,
    pub quota_hours: u32,
    pub quota_warn: bool,
    /// proxy: 0 none, 1 system settings, 2 manual
    pub proxy_mode: u32,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub proxy_user: String,
    /// encrypted on the disk (secret.rs)
    #[serde(with = "crate::secret::string")]
    pub proxy_password: String,
    /// don't use the proxy for these addresses (space separated)
    pub proxy_bypass: String,
    pub logins: Vec<SiteLogin>,
    /// sounds
    pub sounds_on: bool,
    pub sound_complete: String,
    pub sound_failed: String,
    pub sound_queue_start: String,
    pub sound_queue_done: String,
    /// visible list columns (see ALL_COLUMNS) and toolbar look
    pub columns: Vec<String>,
    /// order of the list's columns (dragged in the header), "name" included
    pub column_order: Vec<String>,
    /// width of each column in pixels (dragged in the header)
    pub column_widths: std::collections::BTreeMap<String, u32>,
    pub toolbar_small: bool,
    /// options-on-completion: close running programs when shutting down
    pub force_shutdown: bool,
    /// "Add Aknan Download to your browser" was shown once
    pub extension_prompted: bool,
    /// start downloading at once while "Download File Info" is shown
    pub start_while_info: bool,
    /// "en", "ar", or "" = the language of Windows
    pub language: String,
    /// size of each window the user made bigger / smaller (logical pixels):
    /// "main", "progress"...
    pub window_sizes: std::collections::BTreeMap<String, (f32, f32)>,
    /// the main window was maximized when the program closed
    pub main_maximized: bool,
    /// the progress window shows its details (connections list)
    pub progress_details: bool,
    /// draw the windows with the graphics card (more memory; off = the
    /// processor draws them, works on every computer)
    pub gpu_drawing: bool,
    /// the word about AV1 videos (sound without picture in older players) was shown
    pub av1_tip_shown: bool,
    /// downloads that finished well (for the word about supporting the program)
    pub finished_count: u64,
    /// when that word was shown last (seconds since 1970; 0 = never)
    pub support_asked_at: u64,
    /// the key the browser add-in sends with every request (made at the
    /// first start; encrypted on the disk)
    #[serde(with = "crate::secret::string")]
    pub api_token: String,
    /// the port the add-in really talks to (chosen at random on this
    /// computer; `browser_port` only answers "are you there" and pairing)
    pub api_port: u16,
}
