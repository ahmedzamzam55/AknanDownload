use super::types::{HttpJob, SegTable};
use crate::fileio::{make_sparse, set_hidden};
use crate::http::Progress;
use crate::segments::{self, Segment};
use crate::sync::LockOk;
use std::fs::{File, OpenOptions};
use std::sync::atomic::Ordering;

pub(super) fn open_part_file(job: &HttpJob, segs: &SegTable, progress: &Progress, segmented: bool) -> anyhow::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).read(true).write(true).truncate(false);
    // nobody can delete or move the file while it downloads (Windows let
    // another program, a cleaner or an antivirus, remove it: the download
    // went on into nothing and the file was gone at 100 %); reading it
    // ("Play while downloading") still works
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    let file = opts.open(&job.part_path)?;
    // while it downloads, nobody takes it for the file (it is not yet)
    set_hidden(&job.part_path, true);

    {
        let mut table = segs.lock_ok();
        if segmented {
            let total = job.total_size.unwrap_or(0);
            let fresh = table.is_empty() || file.metadata()?.len() == 0 && segments::downloaded(&table) > 0;
            if fresh {
                *table = segments::plan(total, job.connections, job.min_split);
                file.set_len(0)?;
            }
            if file.metadata()?.len() != total {
                make_sparse(&file);
                file.set_len(total)?;
            }
        } else {
            // no ranges: always start over with one connection
            file.set_len(0)?;
            *table = vec![Segment::new(0, job.total_size.unwrap_or(u64::MAX))];
        }
        for s in table.iter_mut() {
            s.active = false;
            // what is there now was made sure of (the last download ended
            // with it on the disk)
            s.on_disk = Some(s.pos);
        }
        progress
            .bytes
            .store(job.progress_base + segments::downloaded(&table), Ordering::Relaxed);
    }
    Ok(file)
}
