use super::{Entry, Inner, Manager};
use crate::sync::LockOk;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, DownloadView, Status, Step};
use crate::segments;
use std::path::PathBuf;
use std::sync::atomic::Ordering;


/// What a running download offers to play (see `Manager::preview_source`).
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewSource {
    /// one file that grows
    File { path: PathBuf, ready: u64 },
    /// a picture file and a sound file (no sound yet: `audio` is `None`)
    Pair { video: PathBuf, video_ready: u64, audio: Option<PathBuf>, audio_ready: u64 },
}


/// Bytes from the start of a file with no gap.
pub(super) fn ready_from_start(segs: &[segments::Segment]) -> u64 {
    let mut ready = 0u64;
    let mut sorted: Vec<_> = segs.iter().map(|s| (s.start, s.pos)).collect();
    sorted.sort();
    for (start, pos) in sorted {
        if start > ready {
            break;
        }
        ready = ready.max(pos);
    }
    ready
}
pub(super) fn make_view(e: &Entry) -> DownloadView {
    view_of(e, true)
}

pub(super) fn view_of(e: &Entry, with_parts: bool) -> DownloadView {
    let bytes = e.progress.bytes.load(Ordering::Relaxed);
    let mut status = e.rec.status;
    if status == Status::Downloading && e.progress.assembling.load(Ordering::Relaxed) {
        status = Status::Assembling;
    }
    let downloaded = if status == Status::Completed {
        e.rec.total_size.unwrap_or(bytes.max(e.rec.downloaded))
    } else {
        bytes
    };
    let speed = if status.is_running() { e.speed.value } else { 0.0 };
    // the time left: from the steady speed (the speed of this very moment
    // jumps, and a short stop made it disappear)
    let steady = if status.is_running() { e.speed.steady } else { 0.0 };
    let pace = if steady > 1.0 { steady } else { speed };
    let eta_secs = match e.rec.total_size {
        Some(t) if pace > 1.0 => Some((t.saturating_sub(downloaded) as f64 / pace) as u64),
        _ => None,
    };
    let segments = if status == Status::Completed || !with_parts {
        Vec::new()
    } else {
        e.segs.lock_ok().iter().map(|s| (s.start, s.pos, s.end, s.active)).collect()
    };
    DownloadView {
        id: e.rec.id,
        url: e.rec.url.clone(),
        file_name: e.rec.file_name.clone(),
        save_path: e.rec.target_path(),
        category: e.rec.category.clone(),
        kind: e.rec.kind,
        status,
        error: e.rec.error.clone(),
        total_size: e.rec.total_size,
        downloaded,
        speed,
        eta_secs,
        active_connections: e.progress.connections.load(Ordering::Relaxed),
        seeders: e.progress.seeders.load(Ordering::Relaxed),
        waiting: status.is_running()
            && e.progress.waiting.load(Ordering::Relaxed)
            && e.progress.connections.load(Ordering::Relaxed) == 0,
        resumable: e.rec.resumable,
        parts_total: e.progress.parts_total.load(Ordering::Relaxed),
        parts_done: e.progress.parts_done.load(Ordering::Relaxed),
        segments,
        queue: e.rec.queue,
        finished_at: e.rec.finished_at,
        description: e.rec.description.clone(),
        group: e.rec.group,
        checksum: e.rec.checksum.clone(),
        speed_limit_kb: e.rec.speed_limit_kb,
        on_complete: e.rec.on_complete,
        added_at: e.rec.added_at,
        last_try: e.rec.last_try,
        headers: e.rec.headers.clone(),
        parent_page: e.rec.source_page.clone().or_else(|| e.rec.headers.referer.clone()),
        queue_pos: e.rec.queue_pos,
        source_key: e.rec.source_key.clone(),
        step: Step::from_code(e.progress.step.load(Ordering::Relaxed))
            .filter(|_| status.is_running())
            .map(|st| (st, e.progress.step_done.load(Ordering::Relaxed) as f32 / 1000.0)),
    }
}
pub(super) fn collect_records(inner: &Inner) -> Vec<DownloadRecord> {
    inner
        .entries
        .values()
        .map(|e| {
            let mut r = e.rec.clone();
            if r.status != Status::Completed {
                // only what is surely on the disk (a power cut loses the rest)
                r.segments = segments::as_on_disk(&e.segs.lock_ok());
                r.audio_segments = segments::as_on_disk(&e.audio_segs.lock_ok());
                r.downloaded = e.progress.bytes.load(Ordering::Relaxed);
                r.parts_total = e.progress.parts_total.load(Ordering::Relaxed);
                r.parts_done = e.progress.parts_done.load(Ordering::Relaxed);
            }
            r
        })
        .collect()
}

impl Manager {
    /// not show them, and copying them several times a second costs).
    pub fn snapshot(&self) -> Vec<DownloadView> {
        let inner = self.inner.lock_ok();
        inner.entries.values().map(|e| view_of(e, false)).collect()
    }

    /// One download (cheap: used many times a second by progress windows).
    /// What can be watched / listened to of a download that is still
    /// running: the part of each file from its start with no gap (players
    /// stop at the first gap). `None`: nothing yet, or a kind that can't be
    /// played before it is done (streams joined at the end, torrents).
    pub fn preview_source(&self, id: DownloadId) -> Option<PreviewSource> {
        let inner = self.inner.lock_ok();
        let e = inner.entries.get(&id)?;
        if e.rec.status == Status::Completed {
            return None;
        }
        match e.rec.kind {
            DownloadKind::Http => {
                let segs = e.segs.lock_ok().clone();
                let ready = if segs.is_empty() { e.progress.bytes.load(Ordering::Relaxed) } else { ready_from_start(&segs) };
                let path = e.rec.part_path();
                (ready > 0 && path.exists()).then_some(PreviewSource::File { path, ready })
            }
            // picture and sound come as two files (YouTube): both are needed
            DownloadKind::Dash => {
                let (video, audio) = (e.rec.video_part_path(), e.rec.audio_part_path());
                let video_ready = ready_from_start(&e.segs.lock_ok());
                let audio_ready = ready_from_start(&e.audio_segs.lock_ok());
                (video_ready > 0 && video.exists()).then(|| PreviewSource::Pair {
                    video,
                    video_ready,
                    audio: (audio_ready > 0 && audio.exists()).then_some(audio),
                    audio_ready,
                })
            }
            DownloadKind::Hls | DownloadKind::Torrent => None,
        }
    }

    pub fn get(&self, id: DownloadId) -> Option<DownloadView> {
        self.inner.lock_ok().entries.get(&id).map(make_view)
    }
}
