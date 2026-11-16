use super::{Manager};
use crate::sync::LockOk;
use crate::config::Settings;
use crate::http::Progress;
use crate::http_download::{Cancelled, SegTable};
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadId, DownloadRecord, Status};
use crate::segments;
use crate::system;
use anyhow::anyhow;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Manager {
    /// A file on an FTP server: its size and date asked (a changed file is
    /// begun again), then fetched in parts when the server allows.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_ftp(
        self: &Arc<Self>,
        id: DownloadId,
        mut rec: DownloadRecord,
        segs: SegTable,
        progress: Arc<Progress>,
        limiter: Arc<SpeedLimiter>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let auth = rec.headers.auth.clone();
        let look = tokio::select! {
            r = crate::ftp::look(&rec.url, auth.as_ref()) => r?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        {
            let mut table = segs.lock_ok();
            let changed = rec.total_size.is_some() && look.size != rec.total_size;
            let other_version = !table.is_empty() && rec.server_time.is_some() && look.modified.is_some() && rec.server_time != look.modified;
            if changed || other_version || !look.rest {
                table.clear();
            }
        }
        rec.total_size = look.size;
        rec.resumable = look.rest && look.size.is_some();
        rec.server_time = look.modified.or(rec.server_time);
        rec.final_url = Some(rec.url.clone());
        {
            let r = rec.clone();
            self.update(id, move |x| {
                x.total_size = r.total_size;
                x.resumable = r.resumable;
                x.server_time = r.server_time;
                x.final_url = r.final_url;
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }
        if let Some(total) = rec.total_size {
            let have = segments::downloaded(&segs.lock_ok());
            system::check_room(&rec.work_dir(), total.saturating_sub(have), Some(total)).map_err(|m| anyhow!(m))?;
        }
        let job = crate::ftp::FtpJob {
            url: rec.url.clone(),
            auth,
            path: look.path,
            total_size: rec.total_size,
            rest: look.rest,
            part_path: rec.part_path(),
            connections: rec.connections,
            min_split: settings.min_split(),
            retries: settings.retries,
            limiter,
        };
        crate::ftp::run(self.shared.clone(), job, segs, progress.clone(), cancel.clone()).await?;
        self.finish_file(id, &rec, &progress, &cancel).await
    }

}
