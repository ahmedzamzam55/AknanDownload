use crate::winfx;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use swiftget_engine::{DownloadView, Status};

/// The progress of `v` as the taskbar shows it.
pub(super) fn taskbar_state(v: &DownloadView) -> winfx::TaskbarProgress {
    use winfx::TaskbarProgress as P;
    let amount = match v.total_size {
        Some(t) if t > 0 => Some((v.downloaded.min(t), t)),
        _ if v.parts_total > 0 => Some((v.parts_done.min(v.parts_total) as u64, v.parts_total as u64)),
        _ => None,
    };
    match (v.status, amount) {
        // waiting for the internet: yellow, like paused
        (_, Some((d, t))) if v.waiting => P::Paused(d, t),
        (Status::Failed, Some((d, t))) => P::Error(d, t),
        (Status::Failed, None) => P::Error(1, 1),
        (Status::Paused | Status::Queued, Some((d, t))) => P::Paused(d, t),
        (Status::Paused | Status::Queued, None) => P::Off,
        (_, Some((d, t))) => P::Normal(d, t),
        (_, None) => P::Busy,
    }
}

/// The progress ring of the download window (a circle of radius 43 in a
/// 100 x 100 box, from the top, clockwise).
pub(super) fn ring_path(p: f64) -> String {
    let p = p.clamp(0.0, 1.0);
    if p <= 0.001 {
        return String::new();
    }
    if p >= 0.999 {
        return "M 50 7 A 43 43 0 1 1 50 93 A 43 43 0 1 1 50 7".into();
    }
    let a = p * std::f64::consts::TAU;
    let (x, y) = (50.0 + 43.0 * a.sin(), 50.0 - 43.0 * a.cos());
    format!("M 50 7 A 43 43 0 {} 1 {x:.2} {y:.2}", u8::from(p > 0.5))
}

thread_local! {
    // the speeds of the last minute of each open download window
    static SPEEDS: RefCell<HashMap<u64, VecDeque<f64>>> = RefCell::new(HashMap::new());
}
const SPEED_SAMPLES: usize = 60;

pub(super) fn retain_speeds(ids: &[u64]) {
    SPEEDS.with(|m| m.borrow_mut().retain(|id, _| ids.contains(id)));
}

/// The speed of the last minute as a line and the area under it (200 x 30).
pub(super) fn speed_drawing(id: u64, speed: f64) -> (String, String) {
    SPEEDS.with(|m| {
        let mut m = m.borrow_mut();
        let q = m.entry(id).or_default();
        q.push_back(speed.max(0.0));
        while q.len() > SPEED_SAMPLES {
            q.pop_front();
        }
        speed_paths(q.iter().copied())
    })
}

pub(super) fn speed_paths(values: impl Iterator<Item = f64>) -> (String, String) {
    let v: Vec<f64> = values.collect();
    if v.len() < 2 {
        return (String::new(), String::new());
    }
    let top = v.iter().copied().fold(0.0, f64::max).max(1.0);
    let step = 200.0 / (SPEED_SAMPLES - 1) as f64;
    // the newest on the right
    let x0 = 200.0 - step * (v.len() - 1) as f64;
    let pts: Vec<(f64, f64)> = v.iter().enumerate().map(|(i, s)| (x0 + step * i as f64, 29.0 - 26.0 * s / top)).collect();
    let mut line = format!("M {:.1} {:.1}", pts[0].0, pts[0].1);
    for (x, y) in &pts[1..] {
        line += &format!(" L {x:.1} {y:.1}");
    }
    let area = format!("{line} L 200 30 L {:.1} 30 Z", pts[0].0);
    (line, area)
}
