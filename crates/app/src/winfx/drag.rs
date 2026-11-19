#[cfg(windows)]
use super::opacity::hwnd_of;
use std::path::{Path, PathBuf};

/// Outcome of an Explorer drag-and-drop operation initiated from the app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DragOutcome {
    /// True if at least one dragged item was actually moved away by the drop target.
    pub(crate) moved: bool,
}

#[cfg(windows)]
struct OleGuard(bool);

#[cfg(windows)]
impl OleGuard {
    fn acquire() -> Self {
        let hr = unsafe { windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null_mut()) };
        Self(hr >= 0)
    }
}

#[cfg(windows)]
impl Drop for OleGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { windows_sys::Win32::System::Ole::OleUninitialize() };
        }
    }
}

pub(crate) fn filter_existing_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.exists() && !out.iter().any(|x| x == p) {
            out.push(p.clone());
        }
    }
    out
}

pub(crate) fn shared_parent(paths: &[PathBuf]) -> Option<PathBuf> {
    let first = paths.first()?.parent()?.to_path_buf();
    if paths.iter().all(|p| p.parent() == Some(first.as_path())) {
        Some(first)
    } else {
        None
    }
}

/// Drag one finished file or folder out of the program. Explorer automatically
/// moves on same-drive drops and copies across drives.
pub(crate) fn drag_file_out(w: &slint::Window, path: &Path) -> DragOutcome {
    drag_paths_out(w, &[path.to_path_buf()])
}

/// Drag one or more finished files/folders out of the program into Explorer or
/// another drop target. Never deletes files in Rust; Windows Explorer executes
/// the move natively on same-drive drops.
pub(crate) fn drag_paths_out(w: &slint::Window, paths: &[PathBuf]) -> DragOutcome {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::core::GUID;
        use windows_sys::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
        use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
        use windows_sys::Win32::UI::Shell::{
            ILCreateFromPathW, ILFindLastID, ILFree, SHCreateDataObject, SHDoDragDrop,
        };
        const IID_IDATA_OBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);

        let existing = filter_existing_paths(paths);
        if existing.is_empty() {
            return DragOutcome::default();
        }
        let Some(hwnd) = hwnd_of(w) else { return DragOutcome::default() };

        let _ole = OleGuard::acquire();
        let _modal = crate::shell::suspend_modals();
        unsafe {
            let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(existing.len());
            for p in &existing {
                let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                let pidl = ILCreateFromPathW(wide.as_ptr());
                if !pidl.is_null() {
                    pidls.push(pidl);
                }
            }
            if pidls.is_empty() {
                return DragOutcome::default();
            }

            let mut parent_to_free: *mut ITEMIDLIST = std::ptr::null_mut();
            let desktop_root: ITEMIDLIST = std::mem::zeroed();
            let (parent_ptr, children): (*const ITEMIDLIST, Vec<*const ITEMIDLIST>) =
                if let Some(dir) = shared_parent(&existing) {
                    let wide_dir: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                    let p_pidl = ILCreateFromPathW(wide_dir.as_ptr());
                    if !p_pidl.is_null() {
                        parent_to_free = p_pidl;
                        let kids = pidls.iter().map(|&p| ILFindLastID(p) as *const ITEMIDLIST).collect();
                        (p_pidl as *const ITEMIDLIST, kids)
                    } else {
                        (&desktop_root as *const ITEMIDLIST, pidls.iter().map(|&p| p as *const ITEMIDLIST).collect())
                    }
                } else {
                    (&desktop_root as *const ITEMIDLIST, pidls.iter().map(|&p| p as *const ITEMIDLIST).collect())
                };

            let mut obj: *mut c_void = std::ptr::null_mut();
            let hr = SHCreateDataObject(
                parent_ptr,
                children.len() as u32,
                children.as_ptr(),
                std::ptr::null_mut(),
                &IID_IDATA_OBJECT,
                &mut obj,
            );
            if hr >= 0 && !obj.is_null() {
                let mut effect = 0u32;
                let allowed = DROPEFFECT_MOVE | DROPEFFECT_COPY | DROPEFFECT_LINK;
                let _ = SHDoDragDrop(hwnd as _, obj as _, std::ptr::null_mut(), allowed, &mut effect);
                let vt = *(obj as *const *const [usize; 3]);
                let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute((*vt)[2]);
                release(obj);
            }

            if !parent_to_free.is_null() {
                ILFree(parent_to_free);
            }
            for pidl in pidls {
                ILFree(pidl);
            }
        }
        DragOutcome { moved: existing.iter().any(|p| !p.exists()) }
    }
    #[cfg(not(windows))]
    {
        let _ = (w, paths);
        DragOutcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_parent_and_existing_filter() {
        let root = std::env::temp_dir().join(format!("aknan-drag-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let a = root.join("a.bin");
        let b = root.join("b.bin");
        let c = root.join("sub").join("c.bin");
        let missing = root.join("missing.bin");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        std::fs::write(&c, b"c").unwrap();

        let kept = filter_existing_paths(&[a.clone(), missing, b.clone(), a.clone()]);
        assert_eq!(kept, vec![a.clone(), b.clone()]);
        assert_eq!(shared_parent(&kept), Some(root.clone()));
        assert_eq!(shared_parent(&[a, c]), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
