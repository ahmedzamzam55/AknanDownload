use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct BrowserQuery {
    pub(super) urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// YouTube style: separate audio stream to merge with `url`
    #[serde(default)]
    pub audio_url: Option<String>,
    /// extra headers the links need (from yt-dlp)
    #[serde(default)]
    pub headers: Option<Vec<(String, String)>>,
    /// YouTube page + quality key, so expired links can be renewed
    #[serde(default)]
    pub source_page: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    /// "Download a part": only from / to (seconds)
    #[serde(default)]
    pub clip: Option<ClipAsk>,
    /// a form's download: what the form sent (base64) and its kind
    #[serde(default)]
    pub post: Option<String>,
    #[serde(default)]
    pub post_type: Option<String>,
    /// start immediately in the main list without hiding behind Download File Info
    #[serde(default)]
    pub direct: bool,
    /// optional batch/group id (e.g. downloading multiple subtitle tracks together)
    #[serde(default)]
    pub group: Option<u64>,
    /// target subtitle format ("srt", "vtt", "txt") when converting a subtitle download
    #[serde(default)]
    pub sub_convert: Option<String>,
    /// optional subtitle track to save next to or burn into this video download
    #[serde(default)]
    pub subtitle: Option<swiftget_engine::SubtitleTask>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipAsk {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResolveRequest {
    pub(super) url: String,
    /// the site's cookies, used only if it asks for a login
    #[serde(default)]
    pub(super) cookie_list: Vec<swiftget_engine::ytdlp::BrowserCookie>,
    /// a stream found on a page: that page
    #[serde(default)]
    pub(super) referer: Option<String>,
    /// captured request headers from the browser (Origin, User-Agent, etc.)
    #[serde(default)]
    pub(super) headers: Vec<(String, String)>,
}

/// "Download all links with Aknan Download" from the browser's right click menu.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinksRequest {
    #[serde(default)]
    pub page: String,
    pub links: Vec<PageLink>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageLink {
    pub url: String,
    #[serde(default)]
    pub text: String,
}

pub enum ApiEvent {
    Add(AddRequest),
    Links(LinksRequest),
    Show,
    /// "Download all" on a playlist page: open the playlist window with it
    Playlist(String),
    /// a .torrent file / magnet link opened from Windows (double click):
    /// the torrent window with it
    OpenTorrent(String),
    /// a browser add-in without the key asks to connect: yes / no
    Pair(std::sync::mpsc::Sender<bool>),
    /// the setup of a new version wants the program closed (it saves
    /// everything first; the downloads go on after the update)
    Quit,
    /// the Aknan Download add-on in Firefox is older than this program
    /// (Firefox keeps its own copy): offer to put the new one in
    OldFirefoxAddon,
    /// a download handed over on the command line of another start
    Cli(crate::cli::CliAsk),
}

/// What the copy of the program that is open already said.
#[derive(Debug, PartialEq)]
pub enum Existing {
    /// nobody (or something else) answers on the port
    None,
    /// it brings its window to the front
    Shown,
    /// it answers, but its windows hang (this is its process id)
    Hangs(u32),
}

/// Which of the program's two ports a request came in on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Door {
    /// the known port (Options, 9614): only "are you there", pairing, and
    /// what this program and its setup ask ("show yourself", "close")
    Front,
    /// the port chosen at random on this computer: everything, signed
    Main,
}
