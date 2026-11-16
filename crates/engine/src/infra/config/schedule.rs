use super::parse_hhmm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AfterDone {
    #[default]
    Nothing,
    Exit,
    Sleep,
    Shutdown,
    Hibernate,
}

impl AfterDone {
    /// Order of every "when done" list: nothing, exit, sleep, shut down, hibernate.
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => AfterDone::Exit,
            2 => AfterDone::Sleep,
            3 => AfterDone::Shutdown,
            4 => AfterDone::Hibernate,
            _ => AfterDone::Nothing,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            AfterDone::Nothing => 0,
            AfterDone::Exit => 1,
            AfterDone::Sleep => 2,
            AfterDone::Shutdown => 3,
            AfterDone::Hibernate => 4,
        }
    }
    /// Needs the countdown window (the computer goes off / to sleep).
    pub fn is_power(self) -> bool {
        matches!(self, AfterDone::Sleep | AfterDone::Shutdown | AfterDone::Hibernate)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM", 24h local time
    pub start: String,
    /// "HH:MM" or empty for no stop time
    pub stop: String,
    /// Monday .. Sunday
    pub days: [bool; 7],
    pub when_done: AfterDone,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            start: "02:00".into(),
            stop: String::new(),
            days: [true; 7],
            when_done: AfterDone::Nothing,
        }
    }
}

/// When a queue starts and stops by itself (the queue's "Schedule" tab).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSchedule {
    /// start the queue automatically
    pub start_enabled: bool,
    /// "HH:MM"
    pub start_time: String,
    /// false: daily on `days`; true: once on `once_date`
    pub once: bool,
    /// "YYYY-MM-DD"
    pub once_date: String,
    /// Monday .. Sunday
    pub days: [bool; 7],
    /// start the queue when Aknan Download starts
    pub on_startup: bool,
    pub stop_enabled: bool,
    pub stop_time: String,
    /// wake the computer from sleep for the start
    #[serde(default)]
    pub wake: bool,
}

impl QueueSchedule {
    /// The next time the queue starts by itself after `now` (local time).
    pub fn next_start(&self, now: chrono::DateTime<chrono::Local>) -> Option<chrono::DateTime<chrono::Local>> {
        use chrono::{Datelike, TimeZone};
        if !self.start_enabled {
            return None;
        }
        let (h, m) = parse_hhmm(&self.start_time)?;
        for d in 0..9 {
            let date = now.date_naive() + chrono::Days::new(d);
            let ok = if self.once {
                self.once_date.trim() == date.format("%Y-%m-%d").to_string()
            } else {
                self.days[date.weekday().num_days_from_monday() as usize]
            };
            if !ok {
                continue;
            }
            let at = chrono::Local.from_local_datetime(&date.and_hms_opt(h, m, 0)?).earliest()?;
            if at > now {
                return Some(at);
            }
        }
        None
    }
}

impl Default for QueueSchedule {
    fn default() -> Self {
        QueueSchedule {
            start_enabled: false,
            start_time: "02:00".into(),
            once: false,
            once_date: String::new(),
            days: [true; 7],
            on_startup: false,
            stop_enabled: false,
            stop_time: "08:00".into(),
            wake: false,
        }
    }
}
