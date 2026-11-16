//! Making sure a finished file is the one the site meant: its checksum (a
//! hash the user pasted, or one the server sent with the file) is worked out
//! again from the bytes on the disk and compared.

use std::io::Read;
use std::path::Path;

/// The kinds of hash a checksum can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha384 => "SHA-384",
            Algo::Sha512 => "SHA-512",
        }
    }
    fn hex_len(self) -> usize {
        match self {
            Algo::Md5 => 32,
            Algo::Sha1 => 40,
            Algo::Sha256 => 64,
            Algo::Sha384 => 96,
            Algo::Sha512 => 128,
        }
    }
    fn from_name(n: &str) -> Option<Algo> {
        let n: String = n.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        Some(match n.as_str() {
            "md5" => Algo::Md5,
            "sha1" => Algo::Sha1,
            "sha256" => Algo::Sha256,
            "sha384" => Algo::Sha384,
            "sha512" => Algo::Sha512,
            _ => return None,
        })
    }
    fn from_len(n: usize) -> Option<Algo> {
        [Algo::Md5, Algo::Sha1, Algo::Sha256, Algo::Sha384, Algo::Sha512].into_iter().find(|a| a.hex_len() == n)
    }
}

/// A checksum to compare with: its kind and its value (lower-case hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub algo: Algo,
    pub hex: String,
}

impl Expected {
    /// What the user pasted: "sha256:ab12...", "SHA-256 ab12...", "ab12..."
    /// (the kind told by its length), or a line of a checksum file
    /// ("ab12...  file.zip"). None when it is no checksum.
    pub fn parse(text: &str) -> Option<Expected> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (named, rest) = match text.split_once([':', '=', ' ']) {
            Some((a, b)) if Algo::from_name(a).is_some() => (Algo::from_name(a), b.trim()),
            _ => (None, text),
        };
        let hex: String = rest.split_whitespace().next().unwrap_or("").trim_start_matches('*').to_ascii_lowercase();
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let algo = named.or_else(|| Algo::from_len(hex.len()))?;
        (hex.len() == algo.hex_len()).then_some(Expected { algo, hex })
    }

    /// As it is kept: "sha256:ab12...".
    pub fn text(&self) -> String {
        let name = match self.algo {
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
        };
        format!("{name}:{}", self.hex)
    }

    /// The checksum a server sends with the whole file: "Repr-Digest" /
    /// "Digest" (any answer), "x-goog-hash" (md5), "Content-MD5" (only with
    /// a whole answer: with a part it is the part's).
    pub fn from_headers(h: &reqwest::header::HeaderMap, whole: bool) -> Option<Expected> {
        let get = |n: &str| h.get(n).and_then(|v| v.to_str().ok());
        for name in ["repr-digest", "digest"] {
            if let Some(v) = get(name) {
                // "sha-256=:base64:, sha-512=:base64:" or "SHA-256=base64"
                let mut best: Option<Expected> = None;
                for item in v.split(',') {
                    let Some((a, b)) = item.split_once('=') else { continue };
                    let Some(algo) = Algo::from_name(a) else { continue };
                    let Some(raw) = base64(b.trim().trim_matches(':')) else { continue };
                    let e = Expected { algo, hex: to_hex(&raw) };
                    if e.hex.len() == algo.hex_len() && best.as_ref().is_none_or(|x| x.algo.hex_len() < e.algo.hex_len()) {
                        best = Some(e);
                    }
                }
                if best.is_some() {
                    return best;
                }
            }
        }
        if let Some(v) = get("x-goog-hash") {
            for item in v.split(',') {
                if let Some(b) = item.trim().strip_prefix("md5=") {
                    if let Some(raw) = base64(b.trim()).filter(|r| r.len() == 16) {
                        return Some(Expected { algo: Algo::Md5, hex: to_hex(&raw) });
                    }
                }
            }
        }
        if whole {
            if let Some(raw) = get("content-md5").and_then(|v| base64(v.trim())).filter(|r| r.len() == 16) {
                return Some(Expected { algo: Algo::Md5, hex: to_hex(&raw) });
            }
        }
        None
    }
}

pub(super) fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Base64 (standard or for addresses) to bytes.
pub fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Works out the hash of `path` and compares it. `at` is told how far it
/// is (0..1). Ok(true): the same.
pub fn matches(path: &Path, want: &Expected, at: &dyn Fn(f64), stop: &dyn Fn() -> bool) -> std::io::Result<bool> {
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len().max(1);
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut md5 = Md5::new();
    let mut ctx = match want.algo {
        Algo::Md5 => None,
        Algo::Sha1 => Some(ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY)),
        Algo::Sha256 => Some(ring::digest::Context::new(&ring::digest::SHA256)),
        Algo::Sha384 => Some(ring::digest::Context::new(&ring::digest::SHA384)),
        Algo::Sha512 => Some(ring::digest::Context::new(&ring::digest::SHA512)),
    };
    loop {
        if stop() {
            return Err(std::io::Error::other("stopped"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        match ctx.as_mut() {
            Some(c) => c.update(&buf[..n]),
            None => md5.update(&buf[..n]),
        }
        done += n as u64;
        at(done as f64 / size as f64);
    }
    let got = match ctx {
        Some(c) => to_hex(c.finish().as_ref()),
        None => to_hex(&md5.finish()),
    };
    Ok(got == want.hex)
}

mod md5;
use md5::Md5;

#[cfg(test)]
mod tests;
