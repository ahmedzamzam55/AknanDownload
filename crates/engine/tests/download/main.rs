//! End-to-end: a local HTTP server with Range support, a slow region to force
//! dynamic splitting, then pause + resume. The result must be byte-identical.

mod busy;
mod expiring;
mod flaky;
mod helpers;
mod hls;
mod limits;
mod retarget;
mod segmented;
mod slow_tail;
mod stalls;
mod versions;
