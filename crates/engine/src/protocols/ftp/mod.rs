//! Files on FTP servers (ftp://) and FTP over TLS (ftps://): the size and
//! date asked, then the file in parts on several connections when the
//! server lets a transfer start in the middle ("REST"), as HTTP downloads.
//!
//! ftp:// is plain FTP; when the server asks for TLS first it is switched on
//! ("AUTH TLS"). ftps:// is TLS from the first byte (port 990), or "AUTH TLS"
//! when the address names port 21.


mod control;
mod look;
mod site;
mod transfer;
mod worker;
#[cfg(test)]
mod tests;

use std::time::Duration;

pub(super) const ANSWER: Duration = Duration::from_secs(30);
pub(super) const MAX_LINE: usize = 8192;

pub fn is_ftp(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("ftp://") || u.starts_with("ftps://")
}

pub use control::FtpReply;
pub use look::{file_name, look, Look};
pub use site::Site;
pub use transfer::{run, FtpJob};
