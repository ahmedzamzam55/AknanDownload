use crate::{browser, grab, options, scheduler, tray};
use crate::{
    AboutDialog, AddDialog, BatchDialog, BrowserDialog, CatRow, CategoryDialog, ColDef, ColumnsDialog,
    CompleteDialog, CountdownDialog, DetailsDialog, DownloadRow, DuplicateDialog, FindDialog, GrabberDialog,
    LinkRow, MainWindow, PlaylistDialog, PropertiesDialog, QueueRow, RefreshDialog, SchedulerDialog, SegView,
    SettingsDialog, TipDialog, TorrentDialog, UpdateDialog,
};
use slint::{VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use swiftget_engine::{AfterDone, Manager, NewDownload, ProbeResult, RequestHeaders};

pub(crate) const APP: &str = "Aknan Download";

/// What the browser extension sends along with a video link.
#[derive(Default, Clone)]
pub(crate) struct VideoExtra {
    /// audio stream merged with the video after download
    pub(crate) audio_url: Option<String>,
    /// page + quality key, used to renew expired links
    pub(crate) source_page: Option<String>,
    pub(crate) source_key: Option<String>,
    /// optional subtitle track or standalone subtitle conversion
    pub(crate) subtitle: Option<swiftget_engine::SubtitleTask>,
    pub(crate) sub_convert: Option<String>,
    pub(crate) sub_body: Option<String>,
}

/// Resolved video options & subtitles shown inside "Download File Info".
#[derive(Clone)]
pub(crate) struct AddMediaState {
    pub(crate) source_url: String,
    pub(crate) info: swiftget_engine::ytdlp::YtInfo,
}

/// State of the "Download File Info" window (shared with background threads).
#[derive(Default)]
pub(crate) struct AddState {
    pub(crate) headers: RequestHeaders,
    pub(crate) extra: VideoExtra,
    pub(crate) media: Option<AddMediaState>,
    /// the URL the window was opened with (video extras only apply to it)
    pub(crate) origin_url: String,
    pub(crate) probe: Option<ProbeResult>,
    pub(crate) probe_url: String,
    pub(crate) probe_seq: u64,
    /// last automatic file name we filled in (so we don't overwrite user edits)
    pub(crate) auto_name: String,
    /// last automatic folder we filled in
    pub(crate) auto_folder: String,
    /// category names in the window's list (General first)
    pub(crate) categories: Vec<String>,
    /// download already running in the background for this window; it stays in
    /// `.sgpart` until Start is pressed
    pub(crate) early_id: Option<u64>,
    /// true when inline conflict prompt was opened from "Start Download"
    pub(crate) conflict_start: bool,
    /// links that came in while the window was busy with another one
    pub(crate) waiting: VecDeque<PendingOpen>,
}

/// A "Download File Info" request waiting for the window.
pub(crate) struct PendingOpen {
    pub(crate) url: String,
    pub(crate) headers: RequestHeaders,
    pub(crate) name_hint: Option<String>,
    pub(crate) extra: VideoExtra,
}

/// Everything background threads need. All fields are Send.
#[derive(Clone)]
pub(crate) struct Ctx {
    pub(crate) mgr: Arc<Manager>,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) main: Weak<MainWindow>,
    pub(crate) add: Weak<AddDialog>,
    pub(crate) add_state: Arc<Mutex<AddState>>,
}

/// A progress window and its connections list (updated in place, so it
/// doesn't flicker when refreshed 10 times a second).
pub(crate) type ProgressWin = (DetailsDialog, Rc<VecModel<SegView>>);

/// A download waiting for the answer of the "Duplicate download link" window.
pub(crate) struct PendingAdd {
    pub(crate) req: NewDownload,
    pub(crate) existing: u64,
}

/// Downloads selected in the list (Ctrl / Shift click, Ctrl+A).
#[derive(Default)]
pub(crate) struct Selection {
    pub(crate) ids: Vec<u64>,
    /// where a Shift click range starts
    pub(crate) anchor: Option<u64>,
}

/// Strong Slint dialog handles kept alive for the lifetime of `Ui`.
pub(crate) struct DialogHandles {
    pub(crate) _add: AddDialog,
    pub(crate) _batch: BatchDialog,
    pub(crate) _settings: SettingsDialog,
    pub(crate) _complete: CompleteDialog,
    pub(crate) _columns: ColumnsDialog,
    pub(crate) _category: CategoryDialog,
    pub(crate) _find: FindDialog,
    pub(crate) _props: PropertiesDialog,
    pub(crate) _dup: DuplicateDialog,
    pub(crate) _countdown: CountdownDialog,
    pub(crate) _sched: SchedulerDialog,
    pub(crate) _grabber: GrabberDialog,
    pub(crate) _tips: TipDialog,
    pub(crate) _refresh: RefreshDialog,
    pub(crate) _playlist: PlaylistDialog,
    pub(crate) _torrent: TorrentDialog,
    pub(crate) _browsers: BrowserDialog,
    pub(crate) _updater: UpdateDialog,
    pub(crate) _about: AboutDialog,
}

/// Things that only live on the UI thread.
#[derive(Clone)]
pub(crate) struct Ui {
    pub(crate) _dialogs: Rc<DialogHandles>,
    pub(crate) ctx: Ctx,
    pub(crate) rows: Rc<VecModel<DownloadRow>>,
    pub(crate) queues: Rc<VecModel<QueueRow>>,
    pub(crate) cats: Rc<VecModel<CatRow>>,
    /// one progress window per download
    pub(crate) progress: Rc<RefCell<HashMap<u64, ProgressWin>>>,
    pub(crate) batch: Weak<BatchDialog>,
    pub(crate) settings_dlg: Weak<SettingsDialog>,
    pub(crate) complete: Weak<CompleteDialog>,
    pub(crate) complete_id: Rc<Cell<Option<u64>>>,
    /// the window shows a whole playlist / batch: its folder
    pub(crate) complete_folder: Rc<RefCell<Option<PathBuf>>>,
    /// playlists and batches with a file finished: one window when all are
    pub(crate) groups_waiting: Rc<RefCell<HashSet<u64>>>,
    pub(crate) columns: Weak<ColumnsDialog>,
    pub(crate) category: Weak<CategoryDialog>,
    /// category being edited (None = a new one)
    pub(crate) editing_category: Rc<RefCell<Option<String>>>,
    pub(crate) find: Weak<FindDialog>,
    pub(crate) props: Weak<PropertiesDialog>,
    pub(crate) props_id: Rc<Cell<Option<u64>>>,
    pub(crate) dup: Weak<DuplicateDialog>,
    pub(crate) pending: Rc<RefCell<Option<PendingAdd>>>,
    pub(crate) countdown: Weak<CountdownDialog>,
    /// (action, seconds left)
    pub(crate) power: Rc<RefCell<Option<(AfterDone, i32)>>>,
    pub(crate) sched: Weak<SchedulerDialog>,
    pub(crate) sched_state: Rc<RefCell<scheduler::SchedState>>,
    pub(crate) grabber: Weak<GrabberDialog>,
    pub(crate) grab_state: Rc<RefCell<grab::GrabUi>>,
    pub(crate) tips: Weak<TipDialog>,
    pub(crate) refresh_dlg: Weak<RefreshDialog>,
    /// download waiting for a new address from the browser
    pub(crate) refresh_id: Rc<Cell<Option<u64>>>,
    pub(crate) options_lists: Rc<RefCell<options::Lists>>,
    pub(crate) sel: Rc<RefCell<Selection>>,
    pub(crate) playlist: Weak<PlaylistDialog>,
    pub(crate) playlist_rows: Rc<VecModel<LinkRow>>,
    pub(crate) torrent: Weak<TorrentDialog>,
    pub(crate) torrent_rows: Rc<VecModel<LinkRow>>,
    pub(crate) browsers: Weak<BrowserDialog>,
    pub(crate) updater: Weak<UpdateDialog>,
    /// the download list's columns (order + widths) as the header shows them
    pub(crate) columns_model: Rc<VecModel<ColDef>>,
    pub(crate) browser_list: Rc<RefCell<Vec<browser::Browser>>>,
    pub(crate) tray: Rc<RefCell<Option<tray::Tray>>>,
}

impl Ui {
    pub(super) fn all_dialogs_alive(&self) -> bool {
        self.ctx.main.upgrade().is_some()
            && self.ctx.add.upgrade().is_some()
            && self.batch.upgrade().is_some()
            && self.settings_dlg.upgrade().is_some()
            && self.complete.upgrade().is_some()
            && self.columns.upgrade().is_some()
            && self.category.upgrade().is_some()
            && self.find.upgrade().is_some()
            && self.props.upgrade().is_some()
            && self.dup.upgrade().is_some()
            && self.countdown.upgrade().is_some()
            && self.sched.upgrade().is_some()
            && self.grabber.upgrade().is_some()
            && self.tips.upgrade().is_some()
            && self.refresh_dlg.upgrade().is_some()
            && self.playlist.upgrade().is_some()
            && self.torrent.upgrade().is_some()
            && self.browsers.upgrade().is_some()
            && self.updater.upgrade().is_some()
            && super::shortcuts::about_alive()
    }
}
