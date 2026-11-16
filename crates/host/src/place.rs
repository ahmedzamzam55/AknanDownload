use serde_json::Value;
use std::path::{Path, PathBuf};

/// Where the program and its settings are (the program writes app.json next
/// to this helper when it puts it there).
pub(crate) struct Place {
    pub(crate) app: Option<PathBuf>,
    pub(crate) settings: Option<PathBuf>,
}

impl Place {
    pub(crate) fn find() -> Place {
        let here = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let told = here
            .as_ref()
            .and_then(|d| std::fs::read(d.join("app.json")).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        let path = |k: &str| told[k].as_str().filter(|s| !s.is_empty()).map(PathBuf::from);
        let settings = path("settings").or_else(|| {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Aknan\AknanDownload\data\settings.json"))
        });
        Place { app: path("app"), settings }
    }

    /// The program's own port and key, as it wrote them last.
    pub(crate) fn link(&self) -> Option<(u16, String)> {
        let b = std::fs::read(self.settings.as_ref()?).ok()?;
        let v: Value = serde_json::from_slice(&b).ok()?;
        let port = v["api_port"].as_u64().filter(|p| *p > 0 && *p <= 65535)? as u16;
        let token = open_key(v["api_token"].as_str()?)?;
        Some((port, token))
    }
}

/// The program keeps its key encrypted for this Windows user ("dpapi:<hex>");
/// this helper runs as the same user, so it can read it (no other can).
pub(crate) fn open_key(stored: &str) -> Option<String> {
    let key = match stored.strip_prefix("dpapi:") {
        Some(hex) => String::from_utf8(unprotect(&from_hex(hex)?)?).ok()?,
        None => stored.to_string(),
    };
    (key.len() >= 32).then_some(key)
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` points at `data`, only read during the call; on success
    // Windows allocates `out`, which is copied and freed once
    unsafe {
        let ok = CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(bytes)
    }
}

#[cfg(not(windows))]
fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}
