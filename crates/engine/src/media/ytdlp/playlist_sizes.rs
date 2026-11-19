use super::cookies::remembered_cookies;
use super::options::YtOption;
use super::resolve::resolve_for;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Maximum playlist entries resolved in the background for exact file sizes.
const MAX_EXACT_FETCH: usize = 25;

/// Estimate byte size of a playlist video from its duration and target quality.
pub fn estimate_size(duration_secs: u64, quality: &str) -> Option<u64> {
    if duration_secs == 0 {
        return None;
    }
    let kbps: u64 = match quality {
        "2160" => 24_000,
        "1440" => 12_000,
        "1080" | "best" => 5_000,
        "720" => 2_600,
        "480" => 1_100,
        "360" => 650,
        "240" => 350,
        "144" => 180,
        "audio" | "mp3" => 140,
        _ => 2_600,
    };
    Some(duration_secs.saturating_mul(kbps).saturating_mul(1000) / 8)
}

/// Resolve options for up to `MAX_EXACT_FETCH` playlist videos in the background
/// with 2 lightweight workers so exact byte sizes update live and remain valid
/// across quality dropdown changes.
pub fn fetch_playlist_sizes(
    entries: Vec<(usize, String)>,
    cancel: CancellationToken,
    on_item: impl Fn(usize, Vec<YtOption>) + Send + Sync + 'static,
) {
    if entries.is_empty() {
        return;
    }
    let queue: VecDeque<(usize, String)> = entries.into_iter().take(MAX_EXACT_FETCH).collect();
    let shared_q = Arc::new(Mutex::new(queue));
    let callback = Arc::new(on_item);
    let workers = 2;
    for _ in 0..workers {
        let q = shared_q.clone();
        let c = cancel.clone();
        let cb = callback.clone();
        std::thread::spawn(move || loop {
            if c.is_cancelled() {
                break;
            }
            let next = q.lock().ok().and_then(|mut guard| guard.pop_front());
            let Some((idx, url)) = next else { break };
            if c.is_cancelled() {
                break;
            }
            let cookies = remembered_cookies(&url);
            if let Ok(info) = resolve_for(&url, cookies.as_deref(), None) {
                if !c.is_cancelled() && !info.options.is_empty() {
                    cb(idx, info.options);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::estimate_size;

    #[test]
    fn estimate_scales_by_duration_and_quality() {
        assert_eq!(estimate_size(0, "720"), None);
        let hd = estimate_size(120, "720").unwrap();
        let fhd = estimate_size(120, "1080").unwrap();
        let mp3 = estimate_size(120, "mp3").unwrap();
        assert_eq!(hd, 120 * 2600 * 1000 / 8);
        assert!(fhd > hd && hd > mp3 && mp3 > 0);
    }
}
