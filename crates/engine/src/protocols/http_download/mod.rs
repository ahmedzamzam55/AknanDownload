//! Multi-connection download of a normal HTTP file.
//!
//! Every connection ("worker") asks the segment table for work, downloads its
//! byte range with an HTTP Range request and writes straight into the right
//! offset of the `.sgpart` file. When a worker finishes it steals half of the
//! biggest remaining segment, so all connections stay busy until the end.


mod fetch;
mod learned;
mod prepare;
mod retry;
mod run;
mod types;
mod worker;
#[cfg(test)]
mod tests;

pub(crate) use learned::learn;
pub use learned::{learned_limit, set_learned, site_of, take_learned_changes};
pub use run::run;
pub use types::{max_request_for, Cancelled, FileChanged, FileCheck, HttpJob, SegTable, FILE_CHANGED};
