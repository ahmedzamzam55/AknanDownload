use std::collections::HashSet;
use std::path::Path;
use crate::{filename, Manager, Status, MAIN_QUEUE};

/// Normalized identity key for a video URL so tracking/playlist params don't
/// prevent matching a previously downloaded video.
pub(super) fn video_key(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if let Ok(u) = url::Url::parse(trimmed) {
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        if host.contains("youtube.com") || host.contains("youtubekids.com") || host == "youtu.be" || host.ends_with(".youtu.be") {
            if let Some(id) = u.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.into_owned()).filter(|s| !s.is_empty()) {
                return format!("yt:{id}");
            }
            let segs: Vec<&str> = u.path_segments().into_iter().flatten().filter(|s| !s.is_empty()).collect();
            if host.ends_with("youtu.be") {
                if let Some(id) = segs.first() {
                    return format!("yt:{id}");
                }
            }
            if segs.len() >= 2 && matches!(segs[0], "shorts" | "live" | "embed") {
                return format!("yt:{}", segs[1]);
            }
        }
    }
    trimmed.to_ascii_lowercase()
}

pub(super) fn completed_keys(mgr: &Manager) -> HashSet<String> {
    let mut keys = HashSet::new();
    for v in mgr.snapshot() {
        if v.status == Status::Completed {
            keys.insert(video_key(&v.url));
            if let Some(p) = v.parent_page.as_deref() {
                keys.insert(video_key(p));
            }
        }
    }
    keys
}

fn strip_num_prefix(stem: &str) -> &str {
    let digits = stem.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && stem[digits..].starts_with(" - ") {
        &stem[digits + 3..]
    } else {
        stem
    }
}

pub(super) fn disk_stems(folder: &Path) -> HashSet<String> {
    let mut stems = HashSet::new();
    let Ok(rd) = std::fs::read_dir(folder) else { return stems };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(".aknan-") || name.ends_with(".sgpart") {
            continue;
        }
        if entry.metadata().map(|m| m.len() == 0).unwrap_or(true) {
            continue;
        }
        let (stem, _) = filename::split_ext(&name);
        if !stem.is_empty() {
            stems.insert(stem.to_ascii_lowercase());
            stems.insert(strip_num_prefix(stem).to_ascii_lowercase());
        }
    }
    stems
}

pub(super) fn is_already_downloaded(keys: &HashSet<String>, stems: &HashSet<String>, url: &str, title: &str) -> bool {
    if keys.contains(&video_key(url)) {
        return true;
    }
    let clean = filename::sanitize(title.trim_start_matches('\u{200F}')).to_ascii_lowercase();
    !clean.is_empty() && stems.contains(&clean)
}

pub(super) fn find_existing_queue(mgr: &Manager, name: &str, folder: &Path) -> Option<u64> {
    mgr.settings()
        .queues
        .into_iter()
        .find(|q| {
            q.id != MAIN_QUEUE
                && (q.folder.as_deref() == Some(folder)
                    || (!name.is_empty() && q.name.eq_ignore_ascii_case(name)))
        })
        .map(|q| q.id)
}

pub(super) fn display_number(idx: usize, total: usize, oldest_first: bool) -> usize {
    if oldest_first {
        total.saturating_sub(idx)
    } else {
        idx + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_video_keys_match_across_variants() {
        let k1 = video_key("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123&index=2");
        let k2 = video_key("https://youtu.be/dQw4w9WgXcQ?t=10");
        let k3 = video_key("https://www.youtubekids.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(k1, "yt:dQw4w9WgXcQ");
        assert_eq!(k1, k2);
        assert_eq!(k1, k3);
    }

    #[test]
    fn numbering_respects_oldest_first() {
        assert_eq!(display_number(0, 10, false), 1);
        assert_eq!(display_number(9, 10, false), 10);
        assert_eq!(display_number(0, 10, true), 10);
        assert_eq!(display_number(9, 10, true), 1);
    }

    #[test]
    fn detects_downloaded_files_with_or_without_number_prefix() {
        let tmp = std::env::temp_dir().join(format!("aknan-pl-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("01 - Lesson One.mp4"), b"video").unwrap();
        let stems = disk_stems(&tmp);
        let keys = HashSet::new();
        assert!(is_already_downloaded(&keys, &stems, "https://example.com/1", "Lesson One"));
        assert!(!is_already_downloaded(&keys, &stems, "https://example.com/2", "Lesson Two"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
