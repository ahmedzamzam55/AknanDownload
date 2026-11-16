use super::{Clip, DownloadId, DownloadKind, RequestHeaders, Status};
use crate::segments::Segment;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted state of one download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub kind: DownloadKind,
    pub total_size: Option<u64>,
    pub resumable: bool,
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Dash downloads: the audio stream and its own segment table
    #[serde(default)]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub audio_segments: Vec<Segment>,
    #[serde(default)]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub source_page: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    #[serde(default)]
    pub queue: Option<u64>,
    /// name was only guessed from the URL; the server's real name wins
    #[serde(default)]
    pub auto_name: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// added together with others (a playlist, a batch)
    #[serde(default)]
    pub group: Option<u64>,
    /// this download's own speed limit in KB/s
    #[serde(default)]
    pub speed_limit_kb: Option<u64>,
    /// shut down / sleep / exit when this download finishes
    #[serde(default)]
    pub on_complete: crate::config::AfterDone,
    /// bytes done (used for HLS and for display after restart)
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub parts_total: u32,
    #[serde(default)]
    pub parts_done: u32,
    pub status: Status,
    pub error: Option<String>,
    #[serde(default)]
    pub headers: RequestHeaders,
    pub connections: u32,
    pub category: String,
    pub added_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// temporary folder for the parts (Options > Save To); None = next to the file
    #[serde(default)]
    pub temp_dir: Option<PathBuf>,
    /// position inside its queue (Queue > Files in the queue: move up / down)
    #[serde(default)]
    pub queue_pos: u64,
    /// last time the download was started
    #[serde(default)]
    pub last_try: Option<DateTime<Utc>>,
    /// server's file date (Last-Modified)
    #[serde(default)]
    pub server_time: Option<DateTime<Utc>>,
    /// the server's name for the version of the file being downloaded
    /// ("ETag"): another one later = the file changed on the server
    #[serde(default)]
    pub etag: Option<String>,
    /// automatic retries done by its queue
    #[serde(default)]
    pub retry_count: u32,
    /// video page whose links are found when it starts (playlists)
    #[serde(default)]
    pub pick_quality: Option<String>,
    /// fixed name of the temporary files: the download started while the
    /// "Download File Info" window was still open, so its final name and
    /// folder may still change
    #[serde(default)]
    pub work_stem: Option<String>,
    /// torrents: the files to get, 1-based ("1,3,4"); None = all
    #[serde(default)]
    pub torrent_select: Option<String>,
    /// torrents: the files it writes, relative to `save_dir` (learnt while
    /// it runs; a magnet link only knows them then)
    #[serde(default)]
    pub torrent_files: Vec<String>,
    /// torrents: trackers the user added
    #[serde(default)]
    pub torrent_trackers: Vec<String>,
    /// torrents: the first and last pieces of each file first
    #[serde(default)]
    pub torrent_first_last: bool,
    /// torrents: check every piece on the disk at the next start
    #[serde(default)]
    pub torrent_recheck: bool,
    /// only this part of the video / sound ("Download a part")
    #[serde(default)]
    pub clip: Option<Clip>,
    /// the checksum the finished file must have ("sha256:..."): the user's,
    /// else the one the server sent
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default)]
    pub subtitle: Option<super::SubtitleTask>,
    #[serde(default)]
    pub sub_convert: Option<String>,
}

impl DownloadRecord {
    pub fn target_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }
    /// where the temporary files live
    pub fn work_dir(&self) -> PathBuf {
        match &self.temp_dir {
            Some(t) if !t.as_os_str().is_empty() => t.clone(),
            _ => self.save_dir.clone(),
        }
    }
    pub fn part_path(&self) -> PathBuf {
        if let Some(stem) = &self.work_stem {
            return self.work_dir().join(format!("{stem}.{}", crate::filename::PART_EXT));
        }
        match &self.temp_dir {
            // ids keep same-named files from different folders apart
            Some(t) if !t.as_os_str().is_empty() => {
                crate::filename::part_path(t, &format!("{}-{}", self.id, self.file_name))
            }
            _ => crate::filename::part_path(&self.save_dir, &self.file_name),
        }
    }
    /// base name of the temporary files (with the id when they share a temp folder)
    pub fn temp_stem(&self) -> String {
        if let Some(stem) = &self.work_stem {
            return stem.clone();
        }
        if self.work_dir() == self.save_dir {
            self.file_name.clone()
        } else {
            format!("{}-{}", self.id, self.file_name)
        }
    }
    pub fn hls_temp_dir(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.sgparts", self.temp_stem()))
    }
    pub fn video_part_path(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.video.sgpart", self.temp_stem()))
    }
    pub fn audio_part_path(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.audio.sgpart", self.temp_stem()))
    }
    /// Torrents: every file it writes (only those: the torrent's folder may
    /// have the name of a folder that was there before).
    pub fn torrent_data_paths(&self) -> Vec<PathBuf> {
        if !self.torrent_files.is_empty() {
            return self.torrent_files.iter().map(|f| self.save_dir.join(f)).collect();
        }
        let Some(info) = std::fs::read(&self.url).ok().and_then(|d| crate::torrent::parse_torrent(&d).ok()) else {
            return Vec::new();
        };
        let single = info.files.len() == 1 && info.files[0].path == info.name;
        info.files
            .iter()
            .map(|f| {
                let rel: PathBuf = f.path.split('/').map(crate::filename::sanitize).collect();
                if single {
                    self.save_dir.join(&self.file_name)
                } else {
                    self.save_dir.join(&self.file_name).join(rel)
                }
            })
            .collect()
    }

    /// Every temporary file/folder this download may leave behind.
    pub fn temp_paths(&self) -> Vec<PathBuf> {
        if self.kind == DownloadKind::Torrent {
            // the notes an older version (aria2) kept of what it has
            return vec![self.save_dir.join(format!("{}.aria2", self.file_name))];
        }
        vec![self.part_path(), self.hls_temp_dir(), self.video_part_path(), self.audio_part_path()]
    }
    /// The file name of a part of a video: "name [01-20 → 03-45].mp4"
    /// (a picture is always an MP4 file: it plays everywhere).
    pub fn clip_name(name: &str, clip: &Clip) -> String {
        let (stem, ext) = crate::filename::split_ext(name);
        let ext = match ext.to_ascii_lowercase().as_str() {
            "mp3" => "mp3",
            "m4a" | "aac" => "m4a",
            "opus" | "ogg" => "ogg",
            _ => "mp4",
        };
        format!("{} [{}].{ext}", stem.trim(), clip.label())
    }
    pub fn request_url(&self) -> &str {
        self.final_url.as_deref().unwrap_or(&self.url)
    }
}
