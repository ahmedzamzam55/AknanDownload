use super::site::{timed, tls, Io, Site};
use super::MAX_LINE;
use crate::http::NetDrop;
use anyhow::{anyhow, bail, Context};
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// One FTP control connection, logged in.
pub(super) struct Control {
    pub(super) io: BufReader<Box<dyn Io>>,
    pub(super) peer: SocketAddr,
    pub(super) tls: bool,
    pub(super) host: String,
}

/// The server's answer: its code and text.
pub(super) struct Reply {
    pub(super) code: u16,
    pub(super) text: String,
}

/// An answer that is not the expected one.
#[derive(Debug)]
pub struct FtpReply {
    pub code: u16,
    pub text: String,
}
impl std::fmt::Display for FtpReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the FTP server answered {} {}", self.code, self.text.trim())
    }
}
impl std::error::Error for FtpReply {}


impl Control {
    pub(super) async fn connect(site: &Site) -> anyhow::Result<Control> {
        let tcp = timed(TcpStream::connect((site.host.as_str(), site.port))).await.context("could not connect to the FTP server")?;
        let _ = tcp.set_nodelay(true);
        let peer = tcp.peer_addr()?;
        let mut io: Box<dyn Io> = Box::new(tcp);
        if site.implicit {
            io = tls(io, &site.host).await?;
        }
        let mut c = Control { io: BufReader::new(io), peer, tls: site.implicit, host: site.host.clone() };
        let hello = c.reply().await?;
        if hello.code != 220 {
            return Err(FtpReply { code: hello.code, text: hello.text }.into());
        }
        if site.explicit {
            c.secure().await?;
        }
        let mut user = c.cmd(&format!("USER {}", site.user)).await?;
        // the server wants TLS before the login
        if !c.tls && matches!(user.code, 530 | 534 | 550) && user.text.to_ascii_lowercase().contains("tls") {
            c.secure().await?;
            user = c.cmd(&format!("USER {}", site.user)).await?;
        }
        let logged = match user.code {
            230 => true,
            331 | 332 => c.cmd(&format!("PASS {}", site.password)).await?.code == 230,
            _ => false,
        };
        if !logged {
            bail!("the FTP server did not accept the user name or password - use \"Use authorization\" or Options > Site Logins");
        }
        if c.tls {
            c.cmd("PBSZ 0").await?;
            let prot = c.cmd("PROT P").await?;
            if prot.code != 200 {
                return Err(FtpReply { code: prot.code, text: prot.text }.into());
            }
        }
        let ty = c.cmd("TYPE I").await?;
        if ty.code != 200 {
            return Err(FtpReply { code: ty.code, text: ty.text }.into());
        }
        Ok(c)
    }

    /// "AUTH TLS", then everything over TLS.
    pub(super) async fn secure(&mut self) -> anyhow::Result<()> {
        let r = self.cmd("AUTH TLS").await?;
        if r.code != 234 {
            bail!("the FTP server does not offer a secure connection ({} {})", r.code, r.text.trim());
        }
        // nothing is left unread in the buffer after "234"
        let plain = std::mem::replace(&mut self.io, BufReader::new(Box::new(tokio::io::duplex(1).0)));
        self.io = BufReader::new(tls(plain.into_inner(), &self.host).await?);
        self.tls = true;
        Ok(())
    }

    pub(super) async fn line(&mut self) -> anyhow::Result<String> {
        let mut buf = Vec::new();
        let n = timed((&mut self.io).take(MAX_LINE as u64).read_until(b'\n', &mut buf)).await?;
        if n == 0 {
            return Err(NetDrop("the FTP server closed the connection").into());
        }
        Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
    }

    pub(super) async fn reply(&mut self) -> anyhow::Result<Reply> {
        let first = self.line().await?;
        let code: u16 = first.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| anyhow!("the FTP server sent something else: {first}"))?;
        let mut text = first.get(4..).unwrap_or("").to_string();
        // "123-..." goes on until a line "123 ..."
        if first.as_bytes().get(3) == Some(&b'-') {
            for _ in 0..1000 {
                let l = self.line().await?;
                text.push('\n');
                text.push_str(&l);
                if l.len() >= 4 && l.as_bytes()[3] == b' ' && l.starts_with(&first[..3]) {
                    break;
                }
            }
        }
        Ok(Reply { code, text })
    }

    pub(super) async fn cmd(&mut self, c: &str) -> anyhow::Result<Reply> {
        timed(self.io.get_mut().write_all(format!("{c}\r\n").as_bytes())).await?;
        timed(self.io.get_mut().flush()).await?;
        self.reply().await
    }

    /// A data connection ("EPSV", else "PASV"): always to the server this
    /// connection talks to, whatever address the answer names.
    pub(super) async fn data(&mut self) -> anyhow::Result<Box<dyn Io>> {
        let r = self.cmd("EPSV").await?;
        let port = if r.code == 229 {
            r.text.split('|').filter_map(|p| p.trim().parse::<u16>().ok()).next_back()
        } else {
            let r = self.cmd("PASV").await?;
            if r.code != 227 {
                return Err(FtpReply { code: r.code, text: r.text }.into());
            }
            let nums: Vec<u16> = r.text.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect();
            // "227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)"
            nums.len().checked_sub(2).and_then(|i| (nums[i] < 256 && nums[i + 1] < 256).then(|| nums[i] * 256 + nums[i + 1]))
        };
        let port = port.filter(|p| *p != 0).ok_or_else(|| anyhow!("the FTP server gave no data connection"))?;
        let tcp = timed(TcpStream::connect((self.peer.ip(), port))).await.context("could not open the FTP data connection")?;
        let io: Box<dyn Io> = Box::new(tcp);
        if self.tls {
            return tls(io, &self.host).await;
        }
        Ok(io)
    }

    /// The file from byte `at` on: its data connection.
    pub(super) async fn open(&mut self, path: &str, at: u64) -> anyhow::Result<Box<dyn Io>> {
        let data = self.data().await?;
        if at > 0 {
            let r = self.cmd(&format!("REST {at}")).await?;
            if r.code != 350 {
                return Err(FtpReply { code: r.code, text: r.text }.into());
            }
        }
        let r = self.cmd(&format!("RETR {path}")).await?;
        if !matches!(r.code, 125 | 150) {
            return Err(FtpReply { code: r.code, text: r.text }.into());
        }
        Ok(data)
    }
}
