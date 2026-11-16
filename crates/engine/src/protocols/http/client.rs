use super::{HttpStatus, SendOrdered};
use crate::sync::LockOk;
use anyhow::Context;
use reqwest::Client;
use std::sync::Mutex;
use std::time::Duration;

/// Proxy part of the settings (Options > Proxy / Socks).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProxyConfig {
    /// 0 none, 1 system, 2 manual
    pub mode: u32,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub bypass: String,
}

impl ProxyConfig {
    pub fn from_settings(s: &crate::config::Settings) -> ProxyConfig {
        ProxyConfig {
            mode: s.proxy_mode,
            host: s.proxy_host.trim().to_string(),
            port: s.proxy_port,
            user: s.proxy_user.clone(),
            password: s.proxy_password.clone(),
            bypass: s.proxy_bypass.clone(),
        }
    }
}

/// How long a server may take to answer a request (or to send the next
/// piece of data) before the request is given up and made again.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

pub fn build_client() -> anyhow::Result<Client> {
    build_client_with(&ProxyConfig { mode: 1, ..Default::default() })
}

pub fn build_client_with(proxy: &ProxyConfig) -> anyhow::Result<Client> {
    // the IPv4 copy (see `client_for`) follows the same proxy
    {
        let mut last = LAST_PROXY.lock_ok();
        if *last != *proxy {
            *last = proxy.clone();
            *V4_CLIENT.lock_ok() = None;
        }
    }
    build_client_family(proxy, false)
}

/// Many file sites give a link for "your IP" (the one the browser had on
/// their page). A computer with IPv6 may reach the file's server on its IPv6
/// address while the page saw its IPv4 one: the server answers 403 to the
/// program, never to the browser or to programs that use IPv4 only. Such a
/// server (refusing, then giving the file over IPv4) is asked over IPv4 from
/// then on.
pub(super) static V4_HOSTS: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);
static LAST_PROXY: std::sync::LazyLock<Mutex<ProxyConfig>> =
    std::sync::LazyLock::new(|| Mutex::new(ProxyConfig { mode: 1, ..Default::default() }));
static V4_CLIENT: Mutex<Option<Client>> = Mutex::new(None);

pub(super) fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

pub(super) fn v4_client() -> Option<Client> {
    let mut c = V4_CLIENT.lock_ok();
    if c.is_none() {
        let proxy = LAST_PROXY.lock_ok().clone();
        // through a proxy of one's own: the proxy decides
        if proxy.mode == 2 && !proxy.host.is_empty() {
            return None;
        }
        *c = build_client_family(&proxy, true).ok();
    }
    c.clone()
}

/// The client to ask `url` with: IPv4 only for servers that need it.
pub fn client_for(client: &Client, url: &str) -> Client {
    let v4 = host_of(url).is_some_and(|h| V4_HOSTS.lock_ok().contains(&h));
    if v4 {
        if let Some(c) = v4_client() {
            return c;
        }
    }
    client.clone()
}

/// The proxy the helper programs (video pages, cuts, torrents) go through,
/// as Options set it: they read it from their surroundings.
static TOOL_PROXY: std::sync::LazyLock<Mutex<ProxyConfig>> =
    std::sync::LazyLock::new(|| Mutex::new(ProxyConfig { mode: 1, ..Default::default() }));

const PROXY_VARS: [&str; 6] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"];

/// The surroundings a helper program is started with so that it uses the
/// proxy of Options: (name, value), None = taken away.
pub fn proxy_env() -> Vec<(&'static str, Option<String>)> {
    let p = TOOL_PROXY.lock_ok().clone();
    env_for(&p)
}

pub(super) fn env_for(p: &ProxyConfig) -> Vec<(&'static str, Option<String>)> {
    match p.mode {
        // no proxy: none from the surroundings either
        0 => {
            let mut v: Vec<_> = PROXY_VARS.iter().map(|k| (*k, None)).collect();
            v.push(("NO_PROXY", Some("*".into())));
            v.push(("no_proxy", Some("*".into())));
            v
        }
        2 if !p.host.is_empty() => {
            let host = p.host.trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/');
            let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
            let login = if p.user.is_empty() { String::new() } else { format!("{}:{}@", enc(&p.user), enc(&p.password)) };
            let url = format!("http://{login}{host}:{}", p.port);
            let bypass = p.bypass.split_whitespace().collect::<Vec<_>>().join(",");
            let mut v: Vec<_> = PROXY_VARS.iter().map(|k| (*k, Some(url.clone()))).collect();
            v.push(("NO_PROXY", Some(bypass.clone())));
            v.push(("no_proxy", Some(bypass)));
            v
        }
        // the system's: the programs find it themselves
        _ => Vec::new(),
    }
}

/// The proxy of Options, as the last client was made with it.
pub fn current_proxy() -> ProxyConfig {
    TOOL_PROXY.lock_ok().clone()
}

/// `cmd` started with the proxy of Options.
pub fn with_proxy(cmd: &mut std::process::Command) {
    for (k, v) in proxy_env() {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
}

fn build_client_family(proxy: &ProxyConfig, ipv4_only: bool) -> anyhow::Result<Client> {
    if !ipv4_only {
        *TOOL_PROXY.lock_ok() = proxy.clone();
    }
    let mut b = Client::builder()
        // Real separate TCP connections. With HTTP/2 all our
        // "connections" would share one TCP connection and be as slow as one.
        .http1_only()
        // "User-Agent:", not "user-agent:": the names written the way browsers
        // and other download programs write them. Cloudflare's protection
        // refuses lowercase names (HTTP 403, "challenge") even with the
        // browser's own cookies and name (samfw.com and many more)
        .http1_title_case_headers()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(20))
        // a server that takes the connection and then says nothing (no
        // answer, or the data stops): the request ends instead of waiting
        // for ever, and the download tries again by itself
        .read_timeout(ANSWER_TIMEOUT)
        .pool_max_idle_per_host(64)
        // redirects are followed by `SendOrdered::send_ordered`, which puts
        // "Host" first again on every step (see there)
        .redirect(reqwest::redirect::Policy::none());
    if ipv4_only {
        b = b.local_address(Some(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)));
    }
    match proxy.mode {
        // 1 = system settings: reqwest reads them by itself
        0 => b = b.no_proxy(),
        2 if !proxy.host.is_empty() => {
            let host = proxy.host.trim_start_matches("http://").trim_start_matches("https://");
            let url = format!("http://{host}:{}", proxy.port);
            let mut p = reqwest::Proxy::all(&url).context("the proxy address is not valid")?;
            if !proxy.user.is_empty() {
                p = p.basic_auth(&proxy.user, &proxy.password);
            }
            let bypass = proxy.bypass.split_whitespace().collect::<Vec<_>>().join(",");
            p = p.no_proxy(reqwest::NoProxy::from_string(&bypass));
            b = b.proxy(p);
        }
        _ => {}
    }
    Ok(b.build()?)
}

/// A small file (a few MB at most) fetched whole, e.g. the signed Firefox
/// add-on of a release.
pub async fn fetch_small(proxy: &ProxyConfig, url: &str) -> anyhow::Result<Vec<u8>> {
    let client = build_client_with(proxy)?;
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(60))
        .send_ordered()
        .await?;
    if !resp.status().is_success() {
        return Err(HttpStatus(resp.status().as_u16()).into());
    }
    Ok(resp.bytes().await?.to_vec())
}
