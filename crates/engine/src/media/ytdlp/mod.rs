//! Video links from any website through yt-dlp.
//!
//! yt-dlp knows how to find the real video files on more than 1800 sites
//! (YouTube, Facebook, Instagram, TikTok, X/Twitter, Vimeo, Dailymotion...)
//! and also has a generic extractor for ordinary pages with a video.
//! We only use it to *find* the links (`yt-dlp -J`); the actual download
//! still runs through our multi-connection engine.

mod build;
mod cookies;
mod formats;
mod options;
mod playlist;
mod resolve;
mod subtitles;
#[cfg(test)]
mod tests_options;
#[cfg(test)]
mod tests_run;

pub use build::{apply_headers, build_options};
pub use cookies::{netscape_cookies, remember_cookies, remembered_cookies, BrowserCookie, LOGIN_NEEDED};
pub use options::{mp3_kbps, quality_tag, set_mp3_kbps, YtInfo, YtOption, MP3_KBPS, MP3_KEY};
pub use playlist::{parse_playlist, pick_option, playlist, PlaylistEntry, PlaylistInfo};
pub use resolve::{drive_file_id, forget, resolve, resolve_for, resolve_key, update_if_due, update_now};
pub use subtitles::fetch_subtitles;
