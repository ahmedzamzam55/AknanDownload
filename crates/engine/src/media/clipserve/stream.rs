use super::Entry;
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Up to `want` bytes of `resp` to ffmpeg; how many went.
pub(super) async fn pass_piece(sock: &mut TcpStream, resp: &mut reqwest::Response, entry: &Entry, want: u64) -> anyhow::Result<u64> {
    let mut sent = 0u64;
    while sent < want {
        let Ok(Some(chunk)) = resp.chunk().await else { break };
        let take = chunk.len().min((want - sent) as usize);
        count(entry, take).await;
        sock.write_all(&chunk[..take]).await?;
        sent += take as u64;
    }
    Ok(sent)
}

/// A whole answer to ffmpeg, without its first `skip` bytes, at most `want`.
pub(super) async fn pass_on(sock: &mut TcpStream, resp: &mut reqwest::Response, entry: &Entry, mut skip: u64, want: u64) -> anyhow::Result<()> {
    let mut sent = 0u64;
    while sent < want {
        let Ok(Some(chunk)) = resp.chunk().await else { break };
        let mut data = &chunk[..];
        if skip > 0 {
            let s = skip.min(data.len() as u64) as usize;
            data = &data[s..];
            skip -= s as u64;
        }
        let take = data.len().min((want - sent).min(usize::MAX as u64) as usize);
        if take == 0 {
            continue;
        }
        count(entry, take).await;
        sock.write_all(&data[..take]).await?;
        sent += take as u64;
    }
    Ok(())
}

async fn count(entry: &Entry, n: usize) {
    entry.shared.limiter.acquire(n).await;
    entry.progress.bytes.fetch_add(n as u64, Ordering::Relaxed);
    entry.progress.online();
}
