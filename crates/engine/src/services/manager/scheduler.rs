use super::{Event, Manager};
use crate::sync::{LockOk, RwOk};
use crate::config::{parse_hhmm, AfterDone, QueueDef};
use crate::model::Status;
use crate::system;
use chrono::{Datelike, Timelike, Utc};
use std::sync::Arc;

impl Manager {
    /// (a minute before it). Set again when the time changes.
    pub(super) fn arm_wake(&self, queues: &[QueueDef]) {
        let now = chrono::Local::now();
        let next = queues
            .iter()
            .filter(|q| q.schedule.wake)
            .filter_map(|q| q.schedule.next_start(now))
            .min()
            .map(|t| t.with_timezone(&Utc) - chrono::Duration::seconds(60));
        let mut w = self.waker.lock_ok();
        match next {
            None => *w = None,
            Some(t) if w.as_ref().map(|(at, _)| *at) != Some(t.timestamp()) => {
                *w = system::Waker::at(t).map(|h| (t.timestamp(), h));
            }
            Some(_) => {}
        }
    }


    /// running or waiting is done.
    pub(super) fn check_all_done(self: &Arc<Self>) {
        let action = {
            let mut inner = self.inner.lock_ok();
            if inner.when_all_done == AfterDone::Nothing {
                return;
            }
            let busy = inner
                .entries
                .values()
                .any(|e| e.task.is_some() || e.rec.status == Status::Queued);
            if busy {
                inner.all_done_seen_active = true;
                return;
            }
            if !inner.all_done_seen_active {
                return;
            }
            inner.all_done_seen_active = false;
            std::mem::take(&mut inner.when_all_done)
        };
        self.after_done(action);
    }

    /// Every queue's own schedule (start / stop times, days, once on a date),
    /// and its "when done" action once all its files are finished.
    pub(super) fn check_schedule(self: &Arc<Self>) {
        let queues = self.settings.read_ok().queues.clone();
        let now = chrono::Local::now();
        let key = (now.ordinal(), now.hour(), now.minute());
        let day = now.weekday().num_days_from_monday() as usize;
        let today = now.format("%Y-%m-%d").to_string();
        let new_minute = {
            let mut inner = self.inner.lock_ok();
            if inner.sched_minute == Some(key) {
                false
            } else {
                inner.sched_minute = Some(key);
                true
            }
        };
        if new_minute {
            self.arm_wake(&queues);
            let hm = Some((now.hour(), now.minute()));
            for q in &queues {
                let sc = &q.schedule;
                let day_ok = if sc.once { sc.once_date.trim() == today } else { sc.days[day] };
                if sc.start_enabled && day_ok && parse_hhmm(&sc.start_time) == hm {
                    self.start_queue(q.id);
                }
                if sc.stop_enabled && parse_hhmm(&sc.stop_time) == hm {
                    self.stop_queue(q.id);
                }
            }
        }
        // queues that finished all their files
        let mut done = Vec::new();
        {
            let mut inner = self.inner.lock_ok();
            let armed: Vec<u64> = inner.queue_armed.iter().copied().collect();
            for q in armed {
                let busy = inner
                    .entries
                    .values()
                    .any(|e| e.rec.queue == Some(q) && (e.task.is_some() || e.rec.status == Status::Queued));
                if !busy {
                    inner.queue_armed.remove(&q);
                    inner.events.push(Event::QueueDone(q));
                    done.push(q);
                }
            }
        }
        for q in done {
            if let Some(def) = queues.iter().find(|d| d.id == q) {
                self.after_done(def.after_done);
            }
        }
    }

    /// Exit at once; shut down / sleep / hibernate go through the app's
    /// countdown window (it calls `system::power` when the time is up).
    pub(super) fn after_done(&self, action: AfterDone) {
        match action {
            AfterDone::Nothing => {}
            AfterDone::Exit => self.inner.lock_ok().exit_requested = true,
            AfterDone::Sleep | AfterDone::Shutdown | AfterDone::Hibernate => {
                self.inner.lock_ok().power_request = Some(action);
            }
        }
    }
}
