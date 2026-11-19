use super::finish::copy_watched;
use crate::model::DownloadId;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Move `part` onto `dir/name` atomically on the same volume, replacing any
/// existing file only at swap time (keeps the old file intact on failure).
/// Returns `(_, false)` when the source and destination are on different drives.
pub(super) fn replace_into_place(part: &Path, dir: &Path, name: &str) -> (PathBuf, bool) {
    let target = dir.join(name);
    let mut wait = 100;
    for _ in 0..12 {
        match swap_same_volume(part, &target) {
            Ok(()) => return (target, true),
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33)) && part.exists() => {
                std::thread::sleep(Duration::from_millis(wait));
                wait = (wait * 2).min(1000);
            }
            Err(_) => return (target, false),
        }
    }
    (target, false)
}

#[cfg(windows)]
fn swap_same_volume(from: &Path, target: &Path) -> std::io::Result<()> {
    if !target.exists() {
        return std::fs::rename(from, target);
    }
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, REPLACEFILE_IGNORE_MERGE_ERRORS,
    };
    let w_from: Vec<u16> = from.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let w_to: Vec<u16> = target.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: both wide paths are NUL-terminated for the duration of the call
    let ok = unsafe {
        ReplaceFileW(
            w_to.as_ptr(),
            w_from.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: both wide paths are NUL-terminated for the duration of the call
    let ok = unsafe { MoveFileExW(w_from.as_ptr(), w_to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn swap_same_volume(from: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(from, target)
}

/// Copy `from` to a sibling temp file in `target`'s folder, flush it, then swap
/// atomically onto `target`. On error or cancel, the existing `target` stays intact.
pub(super) fn cross_drive_swap(
    from: &Path,
    target: &Path,
    id: DownloadId,
    on: &dyn Fn(f64),
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let sibling = dir.join(format!(".aknan-swap-{id}.tmp"));
    if let Err(e) = copy_watched(from, &sibling, on, cancel) {
        let _ = std::fs::remove_file(&sibling);
        return Err(e);
    }
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&sibling) {
        let _ = f.sync_all();
    }
    if let Err(e) = swap_same_volume(&sibling, target) {
        let _ = std::fs::remove_file(&sibling);
        return Err(e.into());
    }
    let _ = std::fs::remove_file(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_old_file_on_cancel_and_swaps_atomically() {
        let dir = std::env::temp_dir().join(format!("aknan-swap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("movie.mp4");
        let part = dir.join(".movie.mp4.sgpart");
        std::fs::write(&old, b"old-content").unwrap();
        std::fs::write(&part, b"new-content").unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();
        let src_other = dir.join("other.sgpart");
        std::fs::write(&src_other, b"new-other").unwrap();
        assert!(cross_drive_swap(&src_other, &old, 99, &|_| {}, &cancel).is_err());
        assert_eq!(std::fs::read(&old).unwrap(), b"old-content");
        assert!(!dir.join(".aknan-swap-99.tmp").exists());

        let (swapped, moved) = replace_into_place(&part, &dir, "movie.mp4");
        assert!(moved);
        assert_eq!(swapped, old);
        assert_eq!(std::fs::read(&old).unwrap(), b"new-content");
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
