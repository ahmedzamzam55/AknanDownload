//! Windows bits: free disk space, file system type, shut down / sleep /
//! hibernate, file dates and the virus checker.

use crate::config::AfterDone;
use std::path::Path;

/// Free bytes on the disk that holds `dir` (None when unknown).
pub fn free_space(dir: &Path) -> Option<u64> {
    imp::free_space(dir)
}

/// "NTFS", "FAT32", "exFAT"... of the disk that holds `dir`.
pub fn file_system(dir: &Path) -> Option<String> {
    imp::file_system(dir)
}

/// Is there room for `needed` more bytes in `dir`? Returns a message if not.
pub fn check_room(dir: &Path, needed: u64, total: Option<u64>) -> Result<(), String> {
    let dir = existing_parent(dir);
    if let Some(free) = free_space(&dir) {
        // keep a little air for the file system
        if needed > 0 && free < needed.saturating_add(16 * 1024 * 1024) {
            return Err(format!(
                "Not enough free space on the disk {}: the file needs {} more, only {} free.",
                disk_name(&dir),
                crate::fmt_size(needed),
                crate::fmt_size(free)
            ));
        }
    }
    if let Some(total) = total {
        if total >= 4 * 1024 * 1024 * 1024 {
            if let Some(fs) = file_system(&dir) {
                if fs.eq_ignore_ascii_case("FAT32") || fs.eq_ignore_ascii_case("FAT") {
                    return Err(format!(
                        "This disk uses {fs}, which can't hold files bigger than 4 GB. Save the file to an NTFS or exFAT disk."
                    ));
                }
            }
        }
    }
    Ok(())
}

/// "C:" for C:\\Users\\x\\Downloads, else the folder itself.
pub fn disk_name(dir: &Path) -> String {
    match dir.components().next() {
        Some(std::path::Component::Prefix(p)) => p.as_os_str().to_string_lossy().into_owned(),
        _ => dir.display().to_string(),
    }
}

fn existing_parent(dir: &Path) -> std::path::PathBuf {
    let mut d = dir.to_path_buf();
    while !d.exists() {
        match d.parent() {
            Some(p) if p != d => d = p.to_path_buf(),
            _ => break,
        }
    }
    d
}

/// Give a finished file the server's date.
pub fn set_file_time(path: &Path, t: chrono::DateTime<chrono::Utc>) {
    let ft = filetime::FileTime::from_unix_time(t.timestamp(), 0);
    let _ = filetime::set_file_mtime(path, ft);
}

/// The address without a user name / password and without the "#..." part,
/// for the "downloaded from" note of a file. None when it is not a web address.
fn plain_web_url(u: &str) -> Option<String> {
    let mut url = url::Url::parse(u.trim()).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);
    Some(url.to_string())
}

/// What Windows keeps in `<file>:Zone.Identifier` for a file from the
/// internet (the "Mark of the Web", as browsers write it).
pub fn zone_identifier(host_url: &str, referrer: Option<&str>) -> String {
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referrer.and_then(plain_web_url) {
        out.push_str(&format!("ReferrerUrl={r}\r\n"));
    }
    if let Some(h) = plain_web_url(host_url) {
        out.push_str(&format!("HostUrl={h}\r\n"));
    }
    out
}

/// Mark a downloaded file as coming from the internet, like browsers do:
/// Windows SmartScreen and Office then warn before it is opened or run.
/// Best effort (a disk without NTFS streams, e.g. FAT32, has no place for it).
pub fn mark_from_internet(file: &Path, host_url: &str, referrer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(std::path::PathBuf::from(stream), zone_identifier(host_url, referrer));
    }
    #[cfg(not(windows))]
    {
        let _ = (file, host_url, referrer);
    }
}

/// Run the virus checker on a finished file: `args` with "[File]" replaced.
pub fn virus_scan(program: &str, args: &str, file: &Path) {
    let program = program.trim().trim_matches('"');
    if program.is_empty() {
        return;
    }
    let file = file.display().to_string();
    let args = if args.contains("[File]") {
        args.replace("[File]", &format!("\"{file}\""))
    } else {
        format!("{args} \"{file}\"")
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(split_args(&args));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

/// Split a command line into arguments ("quoted parts" stay together).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Shut down, sleep or hibernate now (after the countdown).
pub fn power(action: AfterDone, force: bool) {
    imp::power(action, force)
}

/// Seconds since the mouse or the keyboard was last used (None: unknown).
pub fn idle_secs() -> Option<u64> {
    imp::idle_secs()
}

/// A wake-up of the computer from sleep at a set time, while it is kept.
pub struct Waker(#[allow(dead_code)] imp::WakeHandle);

impl Waker {
    /// Wake the computer at `at` (Windows' "Allow wake timers" must be on).
    pub fn at(at: chrono::DateTime<chrono::Utc>) -> Option<Waker> {
        imp::wake_at(at).map(Waker)
    }
}

#[cfg(windows)]
#[path = "win.rs"]
mod imp;

#[cfg(not(windows))]
#[path = "fallback.rs"]
mod imp;

#[cfg(test)]
mod tests;
