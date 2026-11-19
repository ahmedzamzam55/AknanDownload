#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
        REG_SZ,
    };

    struct RegKey(HKEY);
    impl Drop for RegKey {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn normalize_subkey(subkey: &str) -> &str {
        subkey
            .strip_prefix(r"HKCU\")
            .or_else(|| subkey.strip_prefix(r"HKEY_CURRENT_USER\"))
            .unwrap_or(subkey)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn value_wide(name: Option<&str>) -> Option<Vec<u16>> {
        name.filter(|s| !s.is_empty()).map(wide)
    }

    pub(crate) fn set_sz(subkey: &str, name: Option<&str>, value: &str) -> bool {
        let key_w = wide(normalize_subkey(subkey));
        let val_w = value_wide(name);
        let data_w = wide(value);
        let mut hkey: HKEY = std::ptr::null_mut();
        unsafe {
            if RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_w.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let _guard = RegKey(hkey);
            let name_ptr = val_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
            RegSetValueExW(
                hkey,
                name_ptr,
                0,
                REG_SZ,
                data_w.as_ptr() as *const u8,
                (data_w.len() * 2) as u32,
            ) == ERROR_SUCCESS
        }
    }

    pub(crate) fn get_sz(subkey: &str, name: Option<&str>) -> Option<String> {
        let key_w = wide(normalize_subkey(subkey));
        let val_w = value_wide(name);
        let mut hkey: HKEY = std::ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
                return None;
            }
            let _guard = RegKey(hkey);
            let name_ptr = val_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
            let mut ty = 0u32;
            let mut bytes = 0u32;
            if RegQueryValueExW(hkey, name_ptr, std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut bytes)
                != ERROR_SUCCESS
                || !matches!(ty, REG_SZ | REG_EXPAND_SZ)
            {
                return None;
            }
            let mut buf = vec![0u16; (bytes as usize / 2) + 1];
            let mut read_bytes = (buf.len() * 2) as u32;
            if RegQueryValueExW(
                hkey,
                name_ptr,
                std::ptr::null(),
                &mut ty,
                buf.as_mut_ptr() as *mut u8,
                &mut read_bytes,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let units = (read_bytes as usize / 2).min(buf.len());
            let len = buf[..units].iter().position(|&c| c == 0).unwrap_or(units);
            let _keep: *const c_void = hkey as _;
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    pub(crate) fn value_exists(subkey: &str, name: Option<&str>) -> bool {
        let key_w = wide(normalize_subkey(subkey));
        let val_w = value_wide(name);
        let mut hkey: HKEY = std::ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
                return false;
            }
            let _guard = RegKey(hkey);
            let name_ptr = val_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
            RegQueryValueExW(hkey, name_ptr, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
                == ERROR_SUCCESS
        }
    }

    pub(crate) fn delete_value(subkey: &str, name: Option<&str>) -> bool {
        let key_w = wide(normalize_subkey(subkey));
        let val_w = value_wide(name);
        let mut hkey: HKEY = std::ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_WRITE, &mut hkey) != ERROR_SUCCESS {
                return false;
            }
            let _guard = RegKey(hkey);
            let name_ptr = val_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
            RegDeleteValueW(hkey, name_ptr) == ERROR_SUCCESS
        }
    }

    pub(crate) fn delete_tree(subkey: &str) -> bool {
        let key_w = wide(normalize_subkey(subkey));
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key_w.as_ptr()) == ERROR_SUCCESS }
    }
}

#[cfg(not(windows))]
mod imp {
    pub(crate) fn set_sz(_subkey: &str, _name: Option<&str>, _value: &str) -> bool { false }
    pub(crate) fn get_sz(_subkey: &str, _name: Option<&str>) -> Option<String> { None }
    pub(crate) fn value_exists(_subkey: &str, _name: Option<&str>) -> bool { false }
    pub(crate) fn delete_value(_subkey: &str, _name: Option<&str>) -> bool { false }
    pub(crate) fn delete_tree(_subkey: &str) -> bool { false }
}

pub(crate) use imp::{delete_tree, delete_value, get_sz, set_sz, value_exists};

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn round_trip_hkcu_registry() {
        let key = format!(r"Software\Aknan\AknanDownloadTest_{}", std::process::id());
        let _ = delete_tree(&key);
        assert!(set_sz(&key, None, "default-val"));
        assert!(set_sz(&key, Some("Item"), "hello"));
        assert_eq!(get_sz(&key, None).as_deref(), Some("default-val"));
        assert_eq!(get_sz(&key, Some("Item")).as_deref(), Some("hello"));
        assert!(value_exists(&key, Some("Item")));
        assert!(delete_value(&key, Some("Item")));
        assert!(!value_exists(&key, Some("Item")));
        assert!(delete_tree(&key));
        assert_eq!(get_sz(&key, None), None);
    }
}
