#[cfg(windows)]
use super::opacity::hwnd_of;

/// Drag a finished file out of the program: the user drops it on the
/// desktop, a folder, a chat... (a copy; the file stays where it is, so the
/// download list stays right). Returns when the mouse is let go.
pub(crate) fn drag_file_out(w: &slint::Window, path: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::core::GUID;
        use windows_sys::Win32::System::Ole::{OleInitialize, DROPEFFECT_COPY, DROPEFFECT_LINK};
        use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
        use windows_sys::Win32::UI::Shell::{
            ILClone, ILCreateFromPathW, ILFindLastID, ILFree, ILRemoveLastID, SHCreateDataObject, SHDoDragDrop,
        };
        const IID_IDATA_OBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);

        let Some(hwnd) = hwnd_of(w) else { return };
        if !path.is_file() {
            return;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL-terminated path that outlives the calls; the
        // item list and the data object are freed / released exactly once
        unsafe {
            let _ = OleInitialize(std::ptr::null_mut());
            let pidl = ILCreateFromPathW(wide.as_ptr());
            if pidl.is_null() {
                return;
            }
            // the folder + the file's own item inside it: what Explorer itself
            // hands out (a full path item alone gives an object nobody accepts)
            let parent = ILClone(pidl);
            if parent.is_null() {
                ILFree(pidl);
                return;
            }
            ILRemoveLastID(parent);
            let child = ILFindLastID(pidl) as *const ITEMIDLIST;
            let items: [*const ITEMIDLIST; 1] = [child];
            let mut obj: *mut c_void = std::ptr::null_mut();
            let hr = SHCreateDataObject(parent, 1, items.as_ptr(), std::ptr::null_mut(), &IID_IDATA_OBJECT, &mut obj);
            if hr >= 0 && !obj.is_null() {
                let mut effect = 0;
                // no drop source of our own: the shell's standard one; a copy
                // (or a shortcut), never a move, so the list stays right
                let _ = SHDoDragDrop(hwnd as _, obj as _, std::ptr::null_mut(), DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
                // IUnknown::Release is the third entry of every COM object's table
                let vt = *(obj as *const *const [usize; 3]);
                let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute((*vt)[2]);
                release(obj);
            }
            ILFree(parent);
            ILFree(pidl);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (w, path);
    }
}
