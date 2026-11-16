use super::{Event, Inner, limit_for, Manager};
use super::views::collect_records;
use crate::sync::{LockOk, RwOk};
use crate::http_download;
use crate::model::Status;
use crate::store;
use crate::system;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl Manager {
    pub(super) fn follow_use(&self) {
        let s = self.settings.read_ok();
        if !s.limit_only_active || !s.speed_limit_on {
            return;
        }
        let limit = limit_for(&s, system::idle_secs());
        drop(s);
        if self.limit_now.swap(limit, Ordering::Relaxed) != limit {
            self.shared.limiter.set_rate(limit);
        }
    }


    pub(super) fn tick(self: &Arc<Self>) {
        if self.ticks.fetch_add(1, Ordering::Relaxed).is_multiple_of(5) {
            self.follow_use();
        }
        // a queue file waits to be tried again: its time may have come
        let again = self.inner.lock_ok().retry_waiting;
        if again {
            self.pump();
        }
        let to_save = {
            let mut inner = self.inner.lock_ok();
            let now = Instant::now();
            let mut any_running = false;
            let mut fresh_bytes = 0u64;
            for e in inner.entries.values_mut() {
                let bytes = e.progress.bytes.load(Ordering::Relaxed);
                if e.task.is_some() && bytes > e.quota_prev {
                    fresh_bytes += bytes - e.quota_prev;
                }
                e.quota_prev = bytes;
                let m = &mut e.speed;
                if e.task.is_none() {
                    m.samples.clear();
                    m.value = 0.0;
                    m.steady = 0.0;
                    continue;
                }
                any_running = true;
                // a restart/resume can make the counter jump back: start over
                if m.samples.back().map(|(_, b)| bytes < *b).unwrap_or(false) {
                    m.samples.clear();
                }
                m.samples.push_back((now, bytes));
                while m.samples.len() > 2 && now.duration_since(m.samples[0].0) > Duration::from_millis(2000) {
                    m.samples.pop_front();
                }
                if let (Some(first), Some(last)) = (m.samples.front(), m.samples.back()) {
                    let dt = last.0.duration_since(first.0).as_secs_f64();
                    if dt > 0.15 {
                        let rate = (last.1 - first.1) as f64 / dt;
                        // light smoothing on top of the 2 s window
                        m.value = if m.value == 0.0 { rate } else { m.value * 0.5 + rate * 0.5 };
                        m.steady = if m.steady == 0.0 { rate } else { m.steady * 0.98 + rate * 0.02 };
                    }
                }
            }
            self.track_quota(&mut inner, now, fresh_bytes);
            // changes to the list: after 3 s; only progress of running
            // downloads: every 10 s (resume works from there anyway)
            let since = now.duration_since(inner.last_save);
            let due = (inner.dirty && since > Duration::from_secs(3)) || (any_running && since > Duration::from_secs(10));
            if due {
                inner.dirty = false;
                inner.last_save = now;
                Some(collect_records(&inner))
            } else {
                None
            }
        };
        if let Some(map) = http_download::take_learned_changes() {
            if let Ok(text) = serde_json::to_string_pretty(&map) {
                let _ = std::fs::write(self.data_dir.join("server-limits.json"), text);
            }
        }
        if let Some(records) = to_save {
            let _ = store::save_downloads(&self.data_dir.join("downloads.json"), &records);
        }
        self.pump();
        self.check_schedule();
        self.check_all_done();
    }

    /// Options > Connection > "Download no more than X MB every Y hours".
    pub(super) fn track_quota(&self, inner: &mut Inner, now: Instant, fresh: u64) {
        let s = self.settings.read_ok();
        if !s.quota_on || s.quota_mb == 0 {
            inner.quota_log.clear();
            if inner.quota_blocked {
                inner.quota_blocked = false;
            }
            return;
        }
        let window = Duration::from_secs(s.quota_hours.max(1) as u64 * 3600);
        let limit = s.quota_mb * 1024 * 1024;
        drop(s);
        if fresh > 0 {
            match inner.quota_log.back_mut() {
                // one entry per minute is enough
                Some((t, b)) if now.duration_since(*t) < Duration::from_secs(60) => *b += fresh,
                _ => inner.quota_log.push_back((now, fresh)),
            }
        }
        while inner.quota_log.front().map(|(t, _)| now.duration_since(*t) > window).unwrap_or(false) {
            inner.quota_log.pop_front();
        }
        let used: u64 = inner.quota_log.iter().map(|(_, b)| *b).sum();
        if used >= limit && !inner.quota_blocked {
            inner.quota_blocked = true;
            let mut stopped = Vec::new();
            for e in inner.entries.values_mut() {
                if e.task.is_some() || e.rec.status == Status::Queued {
                    if let Some(t) = &e.task {
                        t.cancel();
                    }
                    e.rec.status = Status::Paused;
                    e.rec.error = Some("download limit reached (Options > Connection)".into());
                    stopped.push(e.rec.id);
                }
            }
            inner.quota_paused.extend(stopped);
            inner.events.push(Event::QuotaReached);
            inner.dirty = true;
        } else if used < limit && inner.quota_blocked {
            inner.quota_blocked = false;
            for id in std::mem::take(&mut inner.quota_paused) {
                if let Some(e) = inner.entries.get_mut(&id) {
                    if e.rec.status == Status::Paused {
                        e.rec.status = Status::Queued;
                        e.rec.error = None;
                    }
                }
            }
            inner.dirty = true;
        }
    }

}
