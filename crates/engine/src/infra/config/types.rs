use super::{AfterDone, QueueSchedule};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 9614;
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// A download queue: files added to it download one after another (or a few
/// at a time), optionally all into the same folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueDef {
    pub id: u64,
    pub name: String,
    /// files downloading at the same time in this queue (0 = no limit)
    pub max_parallel: u32,
    /// save every file of this queue here (None = category folders)
    pub folder: Option<PathBuf>,
    #[serde(default)]
    pub schedule: QueueSchedule,
    /// a failed file is tried again this many times
    #[serde(default)]
    pub retries: u32,
    /// when every file of the queue is done
    #[serde(default)]
    pub after_done: AfterDone,
    /// open each file when it finishes
    #[serde(default)]
    pub open_when_done: bool,
}

impl QueueDef {
    pub fn new(id: u64, name: &str, max_parallel: u32, folder: Option<PathBuf>) -> QueueDef {
        QueueDef {
            id,
            name: name.to_string(),
            max_parallel,
            folder,
            schedule: QueueSchedule::default(),
            retries: 0,
            after_done: AfterDone::Nothing,
            open_when_done: false,
        }
    }
}

/// A file category of the sidebar: which file types (and sites) go to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryDef {
    pub name: String,
    /// extensions without the dot
    pub exts: Vec<String>,
    /// files from these sites go here too ("*.example.com" allowed)
    #[serde(default)]
    pub sites: Vec<String>,
    /// shipped with the app (can't be deleted, only edited)
    #[serde(default)]
    pub builtin: bool,
}

pub fn default_categories() -> Vec<CategoryDef> {
    let c = |name: &str, exts: &str| CategoryDef {
        name: name.into(),
        exts: exts.split_whitespace().map(String::from).collect(),
        sites: Vec::new(),
        builtin: true,
    };
    vec![
        c("Compressed", "zip rar 7z tar gz tgz bz2 xz zst iso img cab arj lzh ace"),
        c("Documents", "pdf doc docx xls xlsx ppt pptx txt rtf odt ods odp epub csv md pps"),
        c("Music", "mp3 wav flac aac ogg m4a wma opus mid mka ra"),
        c("Programs", "exe msi msix appx apk xapk dmg deb rpm bat jar"),
        c("Video", "mp4 mkv avi mov wmv flv webm m4v ts 3gp mpg mpeg vob rm rmvb asf ogv"),
        c("Images", "jpg jpeg png gif webp bmp svg tif tiff heic ico psd"),
    ]
}

/// Extensions the browser extension hands to Aknan Download (Options > File Types).
pub const DEFAULT_CAPTURE_EXTS: &str = "3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 DEB DMG EXE GZ GZIP IMG ISO LZH M4A M4V MKA MKV MOV MP3 MP4 MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT PPTX QT R0* R1* RA RAR RM RMVB RPM SEA SIT SITX TAR TIF TIFF TGZ WAV WEBM WMA WMV XAPK XZ Z ZIP ZST DOC DOCX XLS XLSX FLAC OPUS";

/// Server with its own connection count (Options > Connection > Exceptions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerLimit {
    pub host: String,
    pub connections: u32,
}

/// Login for a site (Options > Site Logins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteLogin {
    /// "example.com" or "example.com/members"; "*" allowed
    pub site: String,
    pub user: String,
    /// encrypted on the disk (secret.rs)
    #[serde(with = "crate::secret::string")]
    pub password: String,
}

pub const MAIN_QUEUE: u64 = 1;

/// Bump when defaults change in a way old settings files should pick up.
pub(super) const SETTINGS_VERSION: u32 = 5;

/// Columns of the download list that can be shown or hidden (View > Columns).
pub const ALL_COLUMNS: [&str; 11] =
    ["q", "size", "status", "eta", "speed", "last", "desc", "added", "saveto", "referer", "parent"];
/// Width a column starts with (pixels).
pub fn default_column_width(key: &str) -> u32 {
    match key {
        "name" => 280,
        "q" => 30,
        "size" => 82,
        "status" => 110,
        "eta" => 80,
        "speed" => 92,
        "last" => 140,
        "desc" => 170,
        "added" => 140,
        _ => 190,
    }
}

pub const DEFAULT_COLUMNS: [&str; 7] = ["q", "size", "status", "eta", "speed", "last", "desc"];

