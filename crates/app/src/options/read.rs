use super::{KEYS, RATIOS};
use crate::i18n::t;
use crate::*;

pub(super) fn read(ui: &Ui, d: &SettingsDialog, mut s: Settings) -> Result<Settings, String> {
    let dir = d.get_download_dir().trim().to_string();
    if dir.is_empty() {
        return Err(t("Choose a download folder (Save To)"));
    }
    if d.get_proxy_mode() == 2 && d.get_proxy_host().trim().is_empty() {
        return Err(t("Type the proxy address (Proxy / Server)"));
    }
    // General
    s.language = crate::i18n::LANGS[(d.get_language().max(0) as usize).min(crate::i18n::LANGS.len() - 1)].0.into();
    s.theme = d.get_theme().clamp(0, 12) as u32;
    s.opacity = (d.get_window_opacity().round() as u32).clamp(50, 100);
    s.font_scale = (d.get_text_size().round() as u32).clamp(80, 150);
    s.font_family = crate::FONTS.get(d.get_font_index().max(0) as usize).copied().unwrap_or("").to_string();
    s.background_image = d.get_bg_image().to_string();
    s.background_strength = (d.get_bg_strength().round() as u32).clamp(10, 80);
    s.close_to_tray = d.get_close_to_tray();
    s.gpu_drawing = d.get_gpu_drawing();
    s.clipboard_monitor = d.get_clipboard();
    s.auto_start_clipboard = d.get_auto_clipboard();
    s.double_click = d.get_double_click().clamp(0, 1) as u32;
    s.show_tips = d.get_show_tips();
    s.browser_port = d.get_port().clamp(1024, 65535) as u16;
    s.skip_key = KEYS[d.get_skip_key().clamp(0, 3) as usize].into();
    s.force_key = KEYS[d.get_force_key().clamp(0, 3) as usize].into();
    if d.get_start_with_windows() != autostart::is_enabled() {
        autostart::set(d.get_start_with_windows());
    }
    // File types
    s.capture_exts = d.get_capture_exts().split_whitespace().collect::<Vec<_>>().join(" ");
    s.skip_sites = d.get_skip_sites().trim().to_string();
    s.skip_addresses = d
        .get_skip_addresses()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    s.panel_enabled = d.get_panel_enabled();
    s.panel_skip_sites = d.get_panel_skip_sites().trim().to_string();
    s.panel_min_video_kb = d.get_panel_min_video().max(0) as u64;
    s.panel_min_audio_kb = d.get_panel_min_audio().max(0) as u64;
    // Save to
    s.download_dir = PathBuf::from(dir);
    s.category_folders = d.get_category_folders();
    let temp = d.get_temp_dir().trim().to_string();
    s.temp_dir = (!temp.is_empty()).then(|| PathBuf::from(temp));
    // Downloads
    s.show_start_dialog = d.get_show_start_dialog();
    s.start_while_info = d.get_start_while_info();
    s.show_complete_dialog = d.get_complete_dialog();
    s.complete_each_in_group = d.get_complete_each();
    s.ask_before_remove = d.get_ask_remove();
    s.subtitles = d.get_subtitles();
    s.subtitle_langs = d.get_subtitle_langs().trim().chars().take(60).collect();
    s.mp3_kbps = [128, 192, 256, 320][d.get_mp3_index().clamp(0, 3) as usize];
    s.progress_mode = d.get_progress_mode().clamp(0, 2) as u32;
    s.show_progress_window = s.progress_mode != 2;
    s.duplicate_action = d.get_duplicate_action().clamp(0, 3) as u32;
    s.set_file_date = d.get_set_file_date();
    s.virus_scan = d.get_virus_scan();
    s.virus_program = d.get_virus_program().trim().to_string();
    s.virus_args = d.get_virus_args().trim().to_string();
    let ua = d.get_user_agent().trim().to_string();
    if !ua.is_empty() {
        s.user_agent = ua;
    }
    // Connection
    s.connection_type = d.get_connection_type().clamp(0, 2) as u32;
    s.connections = d.get_connections().clamp(1, 64) as u32;
    s.max_active = d.get_max_active().clamp(0, 100) as u32;
    s.retries = d.get_retries().max(0) as u32;
    s.quota_on = d.get_quota_on();
    s.quota_mb = d.get_quota_mb().max(1) as u64;
    s.quota_hours = d.get_quota_hours().max(1) as u32;
    // Proxy
    s.proxy_mode = d.get_proxy_mode().clamp(0, 2) as u32;
    s.proxy_host = d.get_proxy_host().trim().to_string();
    s.proxy_port = d.get_proxy_port().clamp(1, 65535) as u16;
    s.proxy_user = d.get_proxy_user().trim().to_string();
    s.proxy_password = d.get_proxy_password().to_string();
    s.proxy_bypass = d.get_proxy_bypass().trim().to_string();
    // Lists
    {
        let l = ui.options_lists.borrow();
        s.server_limits = l.servers.clone();
        s.logins = l.logins.clone();
    }
    // Sounds
    s.sounds_on = d.get_sounds_on();
    s.sound_complete = d.get_sound_complete().trim().to_string();
    s.sound_failed = d.get_sound_failed().trim().to_string();
    s.sound_queue_start = d.get_sound_qstart().trim().to_string();
    s.sound_queue_done = d.get_sound_qdone().trim().to_string();
    // Speed limiter
    s.speed_limit_on = d.get_limiter_on();
    s.speed_limit_kb = d.get_speed_limit().max(1) as u64;
    s.limiter_on_startup = d.get_limiter_startup();
    s.limit_only_active = d.get_limit_only_active();
    s.torrent_seed_minutes = d.get_seed_minutes().max(0) as u32;
    s.torrent_seed_ratio = RATIOS.get(d.get_seed_ratio().max(0) as usize).copied().unwrap_or(0.0);
    s.torrent_upload_kb = d.get_torrent_upload().max(0) as u64;
    s.torrent_main = d.get_torrent_main();
    Ok(s)
}
