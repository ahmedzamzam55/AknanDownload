//! End-to-end: a local HTTP server with Range support, a slow region to force
//! dynamic splitting, then pause + resume. The result must be byte-identical.

mod busy;
mod early_hold;
mod expiring;
mod flaky;
mod helpers;
mod hls;
mod hls_overhaul;
mod limits;
mod retarget;
mod segmented;
mod slow_tail;
mod stalls;
mod versions;
