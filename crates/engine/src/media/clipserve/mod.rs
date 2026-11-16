//! "Download a part of a video": ffmpeg reads the video from here, on this
//! computer only (127.0.0.1, an address nobody can guess), and this asks
//! the site for exactly the bytes ffmpeg wants, with the download's own
//! headers / sign-in. ffmpeg jumps to the part asked for and reads only
//! that, so the rest of the video is never downloaded.
//!
//! The site is asked in pieces of a few MB, the next one already on its
//! way while the last one is handed over: big video sites send a piece at
//! a time quickly, but slow down a single endless request.

use crate::sync::LockOk;
use crate::model::RequestHeaders;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::http::{Progress, Shared};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

mod serve;
mod stream;
#[cfg(test)]
mod tests;

use serve::serve;

/// Bytes asked of the site at a time: a small first piece (ffmpeg often
/// reads a little and jumps elsewhere), then bigger ones while it goes on.
pub(super) const FIRST_PIECE: u64 = 256 << 10;
pub(super) const PIECE: u64 = 1 << 20;

/// What one address of ours stands for.
#[derive(Clone)]
pub struct Source {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
}

pub(super) struct Entry {
    pub(super) src: Source,
    pub(super) shared: Arc<Shared>,
    /// what came counts for this download's speed and size
    pub(super) progress: Arc<Progress>,
}

pub(super) type Table = Arc<Mutex<HashMap<String, Arc<Entry>>>>;

static SERVER: std::sync::OnceLock<(u16, Table)> = std::sync::OnceLock::new();

/// An address of ours for one stream; it stops working when this goes.
pub struct Ticket {
    pub url: String,
    token: String,
    table: Table,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.table.lock_ok().remove(&self.token);
    }
}

/// The server runs on a thread of its own for as long as the program does
/// (not tied to whoever asked first).
fn server() -> anyhow::Result<&'static (u16, Table)> {
    if let Some(s) = SERVER.get() {
        return Ok(s);
    }
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let table: Table = Arc::new(Mutex::new(HashMap::new()));
    let t = table.clone();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("aknan-clip").enable_all().build()?;
    std::thread::Builder::new().name("aknan-clip-server".into()).spawn(move || {
        rt.block_on(async move {
            let Ok(listener) = TcpListener::from_std(std_listener) else { return };
            loop {
                let Ok((sock, peer)) = listener.accept().await else { continue };
                // this computer only
                if !peer.ip().is_loopback() {
                    continue;
                }
                // what ffmpeg does not take stays at the site: when it
                // jumps elsewhere, little was fetched for nothing
                let _ = socket2::SockRef::from(&sock).set_send_buffer_size(128 << 10);
                let t = t.clone();
                tokio::spawn(async move {
                    let _ = serve(sock, t).await;
                });
            }
        });
    })?;
    // two asking at once: the first one's server is the one
    let _ = SERVER.set((port, table));
    Ok(SERVER.get().expect("set above"))
}

/// Our address for `src` (as long as the ticket lives).
pub async fn open(shared: Arc<Shared>, src: Source, progress: Arc<Progress>) -> anyhow::Result<Ticket> {
    let (port, table) = server()?;
    let token = crate::secret::random_token();
    table.lock_ok().insert(token.clone(), Arc::new(Entry { src, shared, progress }));
    Ok(Ticket { url: format!("http://127.0.0.1:{port}/c/{token}"), token, table: table.clone() })
}

/// The bytes asked for: "bytes=100-" -> (100, None), "bytes=100-199" -> (100, Some(199)).
pub(super) fn parse_range(v: &str) -> Option<(u64, Option<u64>)> {
    let r = v.trim().strip_prefix("bytes=")?;
    if r.contains(',') {
        return None;
    }
    let (a, b) = r.split_once('-')?;
    let a: u64 = a.trim().parse().ok()?;
    let b = b.trim();
    let b = if b.is_empty() { None } else { Some(b.parse::<u64>().ok()?) };
    if b.is_some_and(|b| b < a) {
        return None;
    }
    Some((a, b))
}

/// "bytes 0-99/1000" -> 1000 (None when the size is not given: "/*").
pub(super) fn total_of(content_range: &str) -> Option<u64> {
    content_range.rsplit_once('/')?.1.trim().parse().ok()
}

pub(super) async fn reply_status(sock: &mut TcpStream, code: &str, extra: &str) -> anyhow::Result<()> {
    let head = format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\n{extra}Connection: close\r\n\r\n");
    sock.write_all(head.as_bytes()).await?;
    Ok(())
}

