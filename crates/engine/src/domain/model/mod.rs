//! Data types shared by the engine and the UI.

mod clip;
mod record;
#[cfg(test)]
mod tests;
mod types;
mod view;

pub use clip::{clock, parse_clock, Clip};
pub use record::DownloadRecord;
pub use types::{DownloadId, DownloadKind, NewDownload, ProbeResult, RequestHeaders, Status, Step, SubtitleTask};
pub use view::{fmt_duration, fmt_size, DownloadView};
