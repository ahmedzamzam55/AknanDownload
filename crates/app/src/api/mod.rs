//! Tiny local HTTP API on 127.0.0.1 used by the browser extension.
//!
//! Only the extension can talk to it: requests must carry the `X-SwiftGet`
//! header (a web page cannot add custom headers to a cross-origin request
//! without a CORS preflight, which we never approve) and any Origin header
//! must belong to a browser extension.

mod auth;
mod client;
mod probe;
mod routes;
mod server;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use client::{bind, bind_main, notify_cli, notify_existing, notify_open_torrent, notify_quit, show_on_own_port};
pub use server::serve;
pub use state::{
    config_json, leave_to_browser, now_secs, torrent_source_ok, ADDIN_SEEN, FRONT_OPEN, HELPER_SEEN, MAIN_PORT,
    REPAIR, TOKEN, UI_ALIVE, UI_WAITS,
};
pub use types::{ApiEvent, Door, Existing, LinksRequest};
