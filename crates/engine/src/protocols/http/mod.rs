//! HTTP client, request building and probing a URL.


mod client;
mod probe;
mod probe_at;
mod refresh;
mod request;
mod status;
#[cfg(test)]
mod tests_passing;
#[cfg(test)]
mod tests_refused;

pub use client::{
    build_client, build_client_with, client_for, current_proxy, fetch_small, proxy_env, with_proxy, ProxyConfig,
    ANSWER_TIMEOUT,
};
pub use probe::{probe, probe_quick};
pub use refresh::{add_cookies, drop_kept, is_bot_check, meta_refresh, refresh_target, take_kept, KEEP_FOR};
pub use request::{request, SendOrdered};
pub(crate) use request::looks_like_hls;
pub use status::{
    http_status_of, is_link_gone, is_network_error, is_passing_error, reconnect_delay, relink_delay,
    retry_after_of, retry_delay, wants_captcha, ConnGuard, HttpRetry, HttpStatus, NetDrop, Progress, Shared, CAPTCHA,
};
