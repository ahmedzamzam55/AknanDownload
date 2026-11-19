use super::{Event, Manager};
use super::views::make_view;
use crate::sync::LockOk;
use crate::config::{QueueDef, MAIN_QUEUE};
use crate::model::{DownloadId, DownloadView, Status};
use std::path::PathBuf;
use std::sync::Arc;

impl Manager {
    /// Unfinished files of a queue in their download order.
    pub fn queue_files(&self, queue: u64) -> Vec<DownloadView> {
        let inner = self.inner.lock_ok();
        let mut v: Vec<DownloadView> = inner
            .entries
            .values()
            .filter(|e| e.rec.queue == Some(queue) && e.rec.status != Status::Completed)
            .map(make_view)
            .collect();
        v.sort_by_key(|x| (x.queue_pos, x.id));
        v
    }

    /// Move a file up (-1) or down (+1) inside its queue.
    pub fn move_in_queue(&self, id: DownloadId, delta: i32) {
        let mut inner = self.inner.lock_ok();
        let Some(q) = inner.entries.get(&id).and_then(|e| e.rec.queue) else { return };
        let mut order: Vec<(u64, DownloadId)> = inner
            .entries
            .values()
            .filter(|e| e.rec.queue == Some(q) && e.rec.status != Status::Completed)
            .map(|e| (e.rec.queue_pos, e.rec.id))
            .collect();
        order.sort();
        let Some(i) = order.iter().position(|(_, x)| *x == id) else { return };
        let j = i as i64 + delta as i64;
        if j < 0 || j >= order.len() as i64 {
            return;
        }
        order.swap(i, j as usize);
        // renumber so positions are unique
        for (n, (_, x)) in order.iter().enumerate() {
            if let Some(e) = inner.entries.get_mut(x) {
                e.rec.queue_pos = n as u64 + 1;
            }
        }
        inner.dirty = true;
    }

    /// Start every unfinished download in a queue (they run one after another,
    /// or as many at a time as the queue allows).
    pub fn start_queue(self: &Arc<Self>, queue: u64) {
        {
            let mut inner = self.inner.lock_ok();
            let mut any = false;
            for e in inner.entries.values_mut() {
                if e.rec.queue == Some(queue) && matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                    e.rec.retry_count = 0;
                }
                any |= e.rec.queue == Some(queue) && (e.rec.status == Status::Queued || e.task.is_some());
            }
            if any {
                inner.queue_armed.insert(queue);
                inner.events.push(Event::QueueStarted(queue));
            }
            inner.dirty = true;
        }
        self.pump();
    }

    /// Is any file of this queue running or waiting?
    pub fn queue_running(&self, queue: u64) -> bool {
        let inner = self.inner.lock_ok();
        inner
            .entries
            .values()
            .any(|e| e.rec.queue == Some(queue) && (e.task.is_some() || e.rec.status == Status::Queued))
    }

    pub fn stop_queue(&self, queue: u64) {
        let mut inner = self.inner.lock_ok();
        inner.queue_armed.remove(&queue);
        for e in inner.entries.values_mut() {
            if e.rec.queue != Some(queue) {
                continue;
            }
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    /// Put a download into a queue (None = take it out of any queue).
    pub fn set_queue(self: &Arc<Self>, id: DownloadId, queue: Option<u64>) {
        {
            let mut inner = self.inner.lock_ok();
            let last = inner
                .entries
                .values()
                .filter(|e| queue.is_some() && e.rec.queue == queue)
                .map(|e| e.rec.queue_pos)
                .max()
                .unwrap_or(0);
            if let Some(e) = inner.entries.get_mut(&id) {
                if e.rec.queue != queue {
                    e.rec.queue = queue;
                    // goes to the end of its new queue
                    e.rec.queue_pos = last.max(e.rec.id) + 1;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    /// Create a queue and return its id.
    pub fn add_queue(self: &Arc<Self>, name: &str, max_parallel: u32, folder: Option<PathBuf>) -> anyhow::Result<u64> {
        let mut s = self.settings();
        let id = s.queues.iter().map(|q| q.id).max().unwrap_or(MAIN_QUEUE) + 1;
        s.queues.push(QueueDef::new(id, name.trim(), max_parallel, folder));
        self.set_settings(s)?;
        Ok(id)
    }

    pub fn update_queue(self: &Arc<Self>, def: QueueDef) -> anyhow::Result<()> {
        let mut s = self.settings();
        if let Some(q) = s.queues.iter_mut().find(|q| q.id == def.id) {
            *q = def;
        }
        self.set_settings(s)
    }

    /// Delete a queue; its downloads move to the main queue.
    pub fn delete_queue(self: &Arc<Self>, id: u64) -> anyhow::Result<()> {
        if id == MAIN_QUEUE {
            anyhow::bail!("the main queue can't be deleted");
        }
        {
            let mut inner = self.inner.lock_ok();
            for e in inner.entries.values_mut() {
                if e.rec.queue == Some(id) {
                    e.rec.queue = Some(MAIN_QUEUE);
                }
            }
            inner.dirty = true;
        }
        let mut s = self.settings();
        s.queues.retain(|q| q.id != id);
        self.set_settings(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NewDownload, Status};

    #[test]
    fn queue_files_and_move_skip_completed() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let root = std::env::temp_dir().join(format!("aknan-qview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mgr = Manager::new(rt.handle().clone(), root.clone()).unwrap();
        let q = mgr.add_queue("Course", 1, None).unwrap();
        let a = mgr.add(NewDownload { url: "https://ex/1.mp4".into(), queue: Some(q), start: false, ..Default::default() });
        let b = mgr.add(NewDownload { url: "https://ex/2.mp4".into(), queue: Some(q), start: false, ..Default::default() });
        let c = mgr.add(NewDownload { url: "https://ex/3.mp4".into(), queue: Some(q), start: false, ..Default::default() });
        mgr.update(a, |r| r.status = Status::Completed);

        let files = mgr.queue_files(q);
        assert_eq!(files.iter().map(|v| v.id).collect::<Vec<_>>(), vec![b, c]);

        mgr.move_in_queue(c, -1);
        let reordered = mgr.queue_files(q);
        assert_eq!(reordered.iter().map(|v| v.id).collect::<Vec<_>>(), vec![c, b]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
