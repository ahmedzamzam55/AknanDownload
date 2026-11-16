use super::{key_index, show_lists, RATIOS};
use crate::i18n::{t, tf};
use crate::*;

pub(super) fn fill(ui: &Ui, d: &SettingsDialog, s: &Settings) {
    // General
    d.set_language(crate::i18n::index_of(crate::i18n::effective(&s.language)) as i32);
    d.set_theme(s.theme as i32);
    d.set_window_opacity(s.opacity.clamp(50, 100) as f32);
    d.set_text_size(s.font_scale.clamp(80, 150) as f32);
    d.set_font_index(crate::FONTS.iter().position(|f| *f == s.font_family).unwrap_or(0) as i32);
    d.set_bg_image(s.background_image.clone().into());
    d.set_bg_strength(s.background_strength.clamp(10, 80) as f32);
    d.set_start_with_windows(autostart::is_enabled());
    d.set_close_to_tray(s.close_to_tray);
    d.set_gpu_drawing(s.gpu_drawing);
    d.set_clipboard(s.clipboard_monitor);
    d.set_auto_clipboard(s.auto_start_clipboard);
    d.set_double_click(s.double_click.min(1) as i32);
    d.set_show_tips(s.show_tips);
    d.set_port(s.browser_port as i32);
    d.set_skip_key(key_index(&s.skip_key));
    d.set_force_key(key_index(&s.force_key));
    // File types
    d.set_capture_exts(s.capture_exts.clone().into());
    d.set_skip_sites(s.skip_sites.clone().into());
    d.set_skip_addresses(s.skip_addresses.join("\n").into());
    d.set_panel_enabled(s.panel_enabled);
    d.set_panel_skip_sites(s.panel_skip_sites.clone().into());
    d.set_panel_min_video(s.panel_min_video_kb.min(100_000) as i32);
    d.set_panel_min_audio(s.panel_min_audio_kb.min(100_000) as i32);
    // Save to
    d.set_download_dir(s.download_dir.display().to_string().into());
    d.set_category_folders(s.category_folders);
    d.set_temp_dir(s.temp_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default().into());
    // Downloads
    d.set_show_start_dialog(s.show_start_dialog);
    d.set_start_while_info(s.start_while_info);
    d.set_complete_dialog(s.show_complete_dialog);
    d.set_complete_each(s.complete_each_in_group);
    d.set_ask_remove(s.ask_before_remove);
    d.set_subtitles(s.subtitles);
    d.set_subtitle_langs(s.subtitle_langs.clone().into());
    d.set_mp3_index(match s.mp3_kbps { 0..=159 => 0, 160..=223 => 1, 224..=287 => 2, _ => 3 });
    d.set_progress_mode(s.progress_mode.min(2) as i32);
    d.set_duplicate_action(s.duplicate_action.min(3) as i32);
    d.set_set_file_date(s.set_file_date);
    d.set_virus_scan(s.virus_scan);
    d.set_virus_program(s.virus_program.clone().into());
    d.set_virus_args(s.virus_args.clone().into());
    d.set_user_agent(s.user_agent.clone().into());
    // Connection
    d.set_connection_type(s.connection_type.min(2) as i32);
    d.set_connections(s.connections() as i32);
    d.set_max_active(s.max_active.min(100) as i32);
    d.set_retries(s.retries.min(100) as i32);
    d.set_server_current(-1);
    d.set_server_host(SharedString::new());
    d.set_quota_on(s.quota_on);
    d.set_quota_mb(s.quota_mb.clamp(1, 10_000_000) as i32);
    d.set_quota_hours(s.quota_hours.clamp(1, 720) as i32);
    d.set_quota_used(
        if s.quota_on {
            tf("Downloaded in the current period: {}", &[&fmt_size(ui.ctx.mgr.quota_used())])
        } else {
            t("When the limit is reached the downloads stop, and go on by themselves later.")
        }
        .into(),
    );
    // Proxy
    d.set_proxy_mode(s.proxy_mode.min(2) as i32);
    d.set_proxy_host(s.proxy_host.clone().into());
    d.set_proxy_port(s.proxy_port.max(1) as i32);
    d.set_proxy_user(s.proxy_user.clone().into());
    d.set_proxy_password(s.proxy_password.clone().into());
    d.set_proxy_bypass(s.proxy_bypass.clone().into());
    // Logins
    d.set_login_current(-1);
    d.set_login_site(SharedString::new());
    d.set_login_user(SharedString::new());
    d.set_login_password(SharedString::new());
    // Sounds
    d.set_sounds_on(s.sounds_on);
    d.set_sound_complete(s.sound_complete.clone().into());
    d.set_sound_failed(s.sound_failed.clone().into());
    d.set_sound_qstart(s.sound_queue_start.clone().into());
    d.set_sound_qdone(s.sound_queue_done.clone().into());
    // Speed limiter
    d.set_limiter_on(s.speed_limit_on);
    d.set_speed_limit(s.speed_limit_kb.clamp(1, 1_000_000) as i32);
    d.set_limiter_startup(s.limiter_on_startup);
    d.set_limit_only_active(s.limit_only_active);
    d.set_seed_minutes(s.torrent_seed_minutes.min(10080) as i32);
    d.set_seed_ratio(RATIOS.iter().position(|r| (*r - s.torrent_seed_ratio).abs() < 0.01).unwrap_or(0) as i32);
    d.set_torrent_upload(s.torrent_upload_kb.min(1_000_000) as i32);
    d.set_torrent_main(s.torrent_main);
    show_lists(ui, d);
}
