use super::{Entry, Manager, SpeedMeter};
use super::resolve::free_name;
use crate::sync::LockOk;
use crate::config::AfterDone;
use crate::filename;
use crate::http::Progress;
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadId, DownloadKind, DownloadRecord, NewDownload, Status};
use crate::torrent;
use chrono::Utc;
use std::sync::{Arc, Mutex};

impl Manager {

    pub fn add(self: &Arc<Self>, mut req: NewDownload) -> DownloadId {
        // an FTP address with its login in it: the login is kept apart (and
        // encrypted), the address without it
        if crate::ftp::is_ftp(&req.url) {
            if let Some(login) = crate::ftp::Site::login_in(&req.url) {
                if req.headers.auth.is_none() {
                    req.headers.auth = Some(login);
                }
                req.url = crate::ftp::Site::without_password(&req.url);
            }
        }
        let settings = self.settings();
        let probe = req.probe.clone();
        let auto_name = req.file_name.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) && req.probe.is_none();
        let wanted = req
            .file_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(filename::sanitize)
            .or_else(|| probe.as_ref().map(|p| p.file_name.clone()))
            .or_else(|| filename::from_url(&req.url).map(|n| filename::sanitize(&n)))
            .unwrap_or_else(|| "download".into());
        // torrents: the name they share under, the category of their biggest file
        let is_torrent = torrent::is_torrent_source(&req.url);
        let torrent_info = (is_torrent && !torrent::is_magnet(&req.url))
            .then(|| std::fs::read(&req.url).ok().and_then(|d| torrent::parse_torrent(&d).ok()))
            .flatten();
        let wanted = if is_torrent && req.file_name.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            torrent_info
                .as_ref()
                .map(|i| i.name.clone())
                .or_else(|| torrent::magnet_name(&req.url))
                .map(|n| filename::sanitize(&n))
                .unwrap_or_else(|| "torrent".into())
        } else {
            wanted
        };
        let category = match torrent_info.as_ref().and_then(|i| i.files.iter().max_by_key(|f| f.size)) {
            Some(big) => settings.category_of(&big.path, ""),
            None => settings.category_of(&wanted, &req.url),
        };
        let queue_folder = req
            .queue
            .and_then(|q| settings.queue(q))
            .and_then(|q| q.folder.clone())
            .filter(|d| !d.as_os_str().is_empty());
        let dir = req
            .save_dir
            .clone()
            .filter(|d| !d.as_os_str().is_empty())
            .or(queue_folder)
            .unwrap_or_else(|| settings.dir_for(&category));
        let _ = std::fs::create_dir_all(&dir);

        let audio_url = req.audio_url.clone().filter(|u| !u.trim().is_empty());
        let kind = if is_torrent {
            DownloadKind::Torrent
        } else if audio_url.is_some() {
            DownloadKind::Dash
        } else {
            probe.as_ref().map(|p| p.kind).unwrap_or_else(|| {
                if crate::http::looks_like_hls(&req.url, None) {
                    DownloadKind::Hls
                } else {
                    DownloadKind::Http
                }
            })
        };

        // a part of a video: its name says which part
        let clip = req.clip.filter(|_| !is_torrent);
        let wanted = match &clip {
            Some(c) => DownloadRecord::clip_name(&wanted, c),
            None => wanted,
        };
        let mut inner = self.inner.lock_ok();
        // a torrent keeps its own name: that is the folder / file it makes
        let file_name = if is_torrent { wanted.clone() } else { free_name(&inner, &dir, &wanted) };
        let id = inner.next_id;
        inner.next_id += 1;
        // our own copy of the .torrent file (the one given may be deleted)
        let mut url = req.url.clone();
        if is_torrent && !torrent::is_magnet(&url) {
            let keep = self.data_dir.join("torrents").join(format!("{id}.torrent"));
            if std::fs::create_dir_all(keep.parent().unwrap_or(&self.data_dir)).is_ok() && std::fs::copy(&url, &keep).is_ok() {
                url = keep.to_string_lossy().into_owned();
            }
        }
        let early = req.early;
        let rec = DownloadRecord {
            id,
            url,
            final_url: probe.as_ref().map(|p| p.final_url.clone()),
            file_name,
            save_dir: dir,
            kind,
            // for Dash the real total (video + audio) is known once it starts
            total_size: match kind {
                DownloadKind::Dash => None,
                DownloadKind::Torrent => torrent_info.as_ref().map(|i| i.total),
                _ => probe.as_ref().and_then(|p| p.total_size),
            },
            resumable: probe.as_ref().map(|p| p.resumable).unwrap_or(false),
            segments: Vec::new(),
            audio_url,
            audio_segments: Vec::new(),
            video_size: None,
            source_page: req.source_page.clone(),
            source_key: req.source_key.clone(),
            queue: req.queue,
            auto_name,
            description: req.description.clone().filter(|d| !d.trim().is_empty()),
            group: req.group,
            speed_limit_kb: None,
            on_complete: AfterDone::Nothing,
            downloaded: 0,
            parts_total: 0,
            parts_done: 0,
            status: if req.start { Status::Queued } else { Status::Paused },
            error: None,
            headers: req.headers,
            connections: req.connections.unwrap_or(settings.connections()).clamp(1, 64),
            category,
            added_at: Utc::now(),
            finished_at: None,
            temp_dir: settings
                .temp_dir
                .clone()
                .filter(|t| !t.as_os_str().is_empty() && std::fs::create_dir_all(t).is_ok()),
            queue_pos: id,
            last_try: None,
            server_time: probe.as_ref().and_then(|p| p.last_modified),
            etag: probe.as_ref().and_then(|p| p.etag.clone()),
            retry_count: 0,
            pick_quality: req.pick_quality.clone(),
            work_stem: None,
            torrent_select: req.torrent_select.clone().filter(|s| !s.trim().is_empty()),
            torrent_files: Vec::new(),
            torrent_trackers: Vec::new(),
            torrent_first_last: false,
            torrent_recheck: false,
            clip,
            checksum: req
                .checksum
                .as_deref()
                .and_then(crate::checksum::Expected::parse)
                .or_else(|| probe.as_ref().and_then(|p| p.digest.as_deref()).and_then(crate::checksum::Expected::parse))
                .map(|e| e.text()),
            subtitle: req.subtitle,
            sub_convert: req.sub_convert,
        };
        let mut rec = rec;
        if early {
            // parts get a fixed name in a fixed folder: name and folder can still change
            rec.work_stem = Some(format!(".aknan-{id}"));
            if rec.temp_dir.is_none() {
                rec.temp_dir = Some(rec.save_dir.clone());
            }
            rec.auto_name = false;
        }
        inner.entries.insert(
            id,
            Entry {
                rec,
                segs: Arc::new(Mutex::new(Vec::new())),
                audio_segs: Arc::new(Mutex::new(Vec::new())),
                limiter: Arc::new(SpeedLimiter::new(0)),
                progress: Arc::new(Progress::default()),
                task: None,
                speed: SpeedMeter::default(),
                quota_prev: 0,
            },
        );
        inner.dirty = true;
        drop(inner);
        self.pump();
        id
    }
}
