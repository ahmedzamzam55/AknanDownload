use super::stream::{pass_on, pass_piece};
use super::{parse_range, reply_status, total_of, Table, FIRST_PIECE, PIECE};
use crate::http::{request, SendOrdered};
use crate::sync::LockOk;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(super) async fn serve(mut sock: TcpStream, table: Table) -> anyhow::Result<()> {
    // the request (ffmpeg's are small)
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(20), sock.read(&mut tmp)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 << 10 {
            return reply_status(&mut sock, "431 Too Large", "").await;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut words = first.split_whitespace();
    let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let only_head = method == "HEAD";
    if method != "GET" && !only_head {
        return reply_status(&mut sock, "405 Method Not Allowed", "").await;
    }
    let token = path.strip_prefix("/c/").unwrap_or("");
    let Some(entry) = table.lock_ok().get(token).cloned() else {
        return reply_status(&mut sock, "404 Not Found", "").await;
    };
    let mut asked: Option<(u64, Option<u64>)> = None;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("range") {
                asked = parse_range(v);
            }
        }
    }
    let (from, until) = asked.unwrap_or((0, None));

    // the first piece: tells how big the file is
    let client = entry.shared.client();
    let piece_end = |at: u64, size: u64| until.map(|u| u.min(at + size - 1)).unwrap_or(at + size - 1);
    let ask = |a: u64, b: Option<u64>| {
        let range = match b {
            Some(b) => format!("bytes={a}-{b}"),
            None => format!("bytes={a}-"),
        };
        request(&client, &entry.src.url, &entry.src.headers, &entry.src.user_agent)
            .header(reqwest::header::RANGE, range)
            .send_ordered()
    };
    let mut resp = match ask(from, Some(piece_end(from, FIRST_PIECE))).await {
        Ok(r) => r,
        Err(_) => return reply_status(&mut sock, "502 Bad Gateway", "").await,
    };
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let cr = resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let extra = if cr.is_empty() { String::new() } else { format!("Content-Range: {cr}\r\n") };
        return reply_status(&mut sock, "416 Range Not Satisfiable", &extra).await;
    }
    if !status.is_success() {
        let code = format!("{} Refused", status.as_u16());
        return reply_status(&mut sock, &code, "").await;
    }
    let partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let total = if partial {
        resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(total_of)
    } else {
        resp.content_length()
    };
    let Some(total) = total.filter(|t| *t > 0) else {
        // size unknown: handed over as it comes, without pieces
        if partial {
            resp = match ask(from, until).await {
                Ok(r) if r.status().is_success() => r,
                _ => return reply_status(&mut sock, "502 Bad Gateway", "").await,
            };
        }
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n");
        sock.write_all(head.as_bytes()).await?;
        if !only_head {
            pass_on(&mut sock, &mut resp, &entry, 0, u64::MAX).await?;
        }
        return Ok(());
    };
    if from >= total {
        return reply_status(&mut sock, "416 Range Not Satisfiable", &format!("Content-Range: bytes */{total}\r\n")).await;
    }
    let last = until.unwrap_or(total - 1).min(total - 1);
    let head = if asked.is_some() {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: {content_type}\r\nContent-Range: bytes {from}-{last}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            last - from + 1
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
        )
    };
    sock.write_all(head.as_bytes()).await?;
    if only_head {
        return Ok(());
    }
    if !partial {
        // the site sends the whole file every time: skip to `from`
        return pass_on(&mut sock, &mut resp, &entry, from, last - from + 1).await;
    }
    // piece after piece, each one bigger; once ffmpeg reads on, the next
    // one is asked for while this one is passed on
    let mut at = from;
    let mut size = FIRST_PIECE;
    let mut current = Some(resp);
    let mut pieces = 0;
    while at <= last {
        let end = piece_end(at, size).min(last);
        let next_at = end + 1;
        let next_size = (size * 2).min(PIECE);
        pieces += 1;
        let next = (next_at <= last && pieces >= 3).then(|| tokio::spawn(ask(next_at, Some(piece_end(next_at, next_size).min(last)))));
        let mut r = match current.take() {
            Some(r) => r,
            None => break,
        };
        let mut tries = 0;
        loop {
            let want = end - at + 1;
            match pass_piece(&mut sock, &mut r, &entry, want).await {
                Ok(got) if got == want => break,
                Ok(got) => {
                    // the site stopped early: the rest of the piece again
                    at += got;
                    tries += 1;
                    if tries > 3 {
                        return Ok(());
                    }
                    r = match ask(at, Some(end)).await {
                        Ok(x) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => x,
                        _ => return Ok(()),
                    };
                }
                // ffmpeg went on elsewhere (closed): nothing more to send
                Err(_) => return Ok(()),
            }
        }
        at = next_at;
        size = next_size;
        if at > last {
            break;
        }
        let fresh = match next {
            Some(n) => match n.await {
                Ok(Ok(x)) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => Some(x),
                _ => None,
            },
            None => None,
        };
        current = match fresh {
            Some(x) => Some(x),
            None => match ask(at, Some(piece_end(at, size).min(last))).await {
                Ok(x) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => Some(x),
                _ => return Ok(()),
            },
        };
    }
    Ok(())
}
