use super::types::{
    default_categories, DEFAULT_CAPTURE_EXTS, DEFAULT_COLUMNS, DEFAULT_PORT, DEFAULT_USER_AGENT,
    MAIN_QUEUE, SETTINGS_VERSION,
};
use super::{QueueDef, Schedule, Settings};
use std::path::{Path, PathBuf};

impl Default for Settings {
    fn default() -> Self {
        let download_dir = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        Settings {
            version: SETTINGS_VERSION,
            download_dir,
            category_folders: true,
            connections: 32,
            max_active: 0,
            speed_limit_kb: 0,
            retries: 10,
            user_agent: DEFAULT_USER_AGENT.into(),
            clipboard_monitor: false,
            show_complete_dialog: true,
            complete_each_in_group: false,
            ask_before_remove: true,
            remove_deletes_files: false,
            mp3_kbps: 192,
            subtitles: false,
            subtitle_langs: "ar en".into(),
            browser_port: DEFAULT_PORT,
            min_split_kb: 256,
            schedule: Schedule::default(),
            show_progress_window: true,
            theme: 0,
            opacity: 100,
            font_scale: 100,
            font_family: String::new(),
            background_image: String::new(),
            background_strength: 45,
            last_version: String::new(),
            speed_limit_on: false,
            limit_only_active: false,
            torrent_seed_minutes: 0,
            torrent_upload_kb: 0,
            torrent_seed_ratio: 0.0,
            torrent_main: true,
            category_dirs: Default::default(),
            hide_categories: false,
            sort_by: "date".into(),
            sort_desc: true,
            close_to_tray: true,
            autostart_set: false,
            queues: vec![QueueDef::new(MAIN_QUEUE, "Main download queue", 1, None)],
            categories: default_categories(),
            auto_start_clipboard: false,
            double_click: 0,
            limiter_on_startup: false,
            show_tips: true,
            tip_index: 0,
            capture_exts: DEFAULT_CAPTURE_EXTS.into(),
            skip_sites: String::new(),
            skip_addresses: Vec::new(),
            skip_key: "alt".into(),
            force_key: "none".into(),
            panel_enabled: true,
            panel_skip_sites: String::new(),
            panel_min_video_kb: 0,
            panel_min_audio_kb: 0,
            temp_dir: None,
            show_start_dialog: true,
            progress_mode: 0,
            duplicate_action: 0,
            set_file_date: false,
            virus_scan: false,
            virus_program: String::new(),
            virus_args: "[File]".into(),
            connection_type: 2,
            server_limits: Vec::new(),
            quota_on: false,
            quota_mb: 2000,
            quota_hours: 24,
            quota_warn: true,
            proxy_mode: 1,
            proxy_host: String::new(),
            proxy_port: 8080,
            proxy_user: String::new(),
            proxy_password: String::new(),
            proxy_bypass: "localhost 127.0.0.1".into(),
            logins: Vec::new(),
            sounds_on: false,
            sound_complete: default_sound("tada.wav"),
            sound_failed: default_sound("Windows Critical Stop.wav"),
            sound_queue_start: default_sound("Windows Notify System Generic.wav"),
            sound_queue_done: default_sound("chimes.wav"),
            columns: DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect(),
            column_order: Vec::new(),
            column_widths: Default::default(),
            toolbar_small: false,
            force_shutdown: false,
            extension_prompted: false,
            start_while_info: true,
            language: String::new(),
            window_sizes: Default::default(),
            main_maximized: false,
            progress_details: false,
            gpu_drawing: false,
            av1_tip_shown: false,
            finished_count: 0,
            support_asked_at: 0,
            api_token: String::new(),
            api_port: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        // broken (the power went off while it was saved): its last good copy
        let mut s: Settings = crate::store::read_json_kept(path).unwrap_or_default();
        if s.version < 2 {
            // v2: full speed by default (32 connections, no limit on downloads)
            s.connections = 32;
            s.max_active = 0;
            s.min_split_kb = 256;
            s.show_progress_window = true;
        }
        if s.version < 3 {
            // v3: the speed limit got an on/off switch
            s.speed_limit_on = s.speed_limit_kb > 0;
            if s.sort_by.is_empty() {
                s.sort_by = "date".into();
                s.sort_desc = true;
            }
        }
        if s.version < 4 {
            // v4: progress window has 3 modes; the old global scheduler
            // becomes the main queue's schedule
            s.progress_mode = if s.show_progress_window { 0 } else { 2 };
            let old = s.schedule.clone();
            if let Some(q) = s.queues.iter_mut().find(|q| q.id == MAIN_QUEUE) {
                if q.name == "Main queue" {
                    q.name = "Main download queue".into();
                }
                if old.enabled {
                    q.schedule.start_enabled = true;
                    q.schedule.start_time = old.start.clone();
                    q.schedule.days = old.days;
                    q.schedule.stop_enabled = !old.stop.trim().is_empty();
                    if q.schedule.stop_enabled {
                        q.schedule.stop_time = old.stop.clone();
                    }
                    q.after_done = old.when_done;
                }
            }
            s.schedule.enabled = false;
        }
        if s.version < 5 {
            // v5: watching the clipboard is off until the user turns it on
            // (it reads everything that is copied)
            s.clipboard_monitor = false;
        }
        s.version = SETTINGS_VERSION;
        if !s.queues.iter().any(|q| q.id == MAIN_QUEUE) {
            s.queues.insert(0, QueueDef::new(MAIN_QUEUE, "Main download queue", 1, None));
        }
        if s.categories.is_empty() {
            s.categories = default_categories();
        }
        if s.columns.is_empty() {
            s.columns = DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect();
        }
        if s.limiter_on_startup {
            s.speed_limit_on = true;
        }
        s
    }
}

fn default_sound(name: &str) -> String {
    if cfg!(windows) {
        format!("C:\\Windows\\Media\\{name}")
    } else {
        String::new()
    }
}
