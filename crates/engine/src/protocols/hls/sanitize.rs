use crate::model::RequestHeaders;
use std::path::PathBuf;
use std::time::Duration;

/// Container format carried inside downloaded HLS segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlsContainer {
    MpegTs,
    Fmp4,
}

/// Result returned when an HLS stream download completes.
#[derive(Debug, Clone)]
pub struct HlsOutcome {
    pub first_at: f64,
    pub container: HlsContainer,
    pub audio_output: Option<PathBuf>,
    pub temp_dir: PathBuf,
}

/// Detect whether `bytes` begins with an ISO BMFF (fMP4) box rather than MPEG-TS.
pub fn detect_container(bytes: &[u8]) -> HlsContainer {
    if bytes.len() >= 8 {
        let tag = &bytes[4..8];
        if matches!(tag, b"ftyp" | b"styp" | b"moof" | b"moov" | b"sidx") {
            return HlsContainer::Fmp4;
        }
    }
    HlsContainer::MpegTs
}

fn has_disguised_preamble(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG\r\n\x1a\n")
        || b.starts_with(b"GIF87a")
        || b.starts_with(b"GIF89a")
        || b.starts_with(b"\xff\xd8\xff")
        || b.starts_with(b"BM")
        || (b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP")
        || b.starts_with(b"ID3")
}

/// Strip disguised image/ID3 wrappers prepended before valid 188-byte MPEG-TS
/// sync packets (`0x47`). Returns `bytes` unchanged if no spoofed header applies.
pub fn strip_ts_preamble(bytes: &[u8]) -> &[u8] {
    if bytes.len() < 189 || bytes[0] == 0x47 || !has_disguised_preamble(bytes) {
        return bytes;
    }
    if detect_container(bytes) == HlsContainer::Fmp4 {
        return bytes;
    }
    let limit = bytes.len().min(64 * 1024);
    for off in 1..limit {
        if bytes[off] != 0x47 {
            continue;
        }
        let ok3 = off + 376 < bytes.len() && bytes[off + 188] == 0x47 && bytes[off + 376] == 0x47;
        let ok2 = off + 376 >= bytes.len() && off + 188 < bytes.len() && bytes[off + 188] == 0x47;
        if ok3 || ok2 {
            return &bytes[off..];
        }
    }
    bytes
}

fn registrable(host: &str) -> &str {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        let tail_start = host.len() - parts[parts.len() - 2].len() - parts[parts.len() - 1].len() - 1;
        &host[tail_start..]
    } else {
        host
    }
}

/// Drop credentialed `cookies` / `authorization` headers when an HLS CDN host
/// differs from the playlist's own host.
pub fn scoped_headers(base: &RequestHeaders, playlist_url: &str, target_url: &str) -> RequestHeaders {
    let host_of = |u: &str| url::Url::parse(u).ok().and_then(|p| p.host_str().map(str::to_ascii_lowercase));
    let same = match (host_of(playlist_url), host_of(target_url)) {
        (Some(a), Some(b)) => a == b || registrable(&a) == registrable(&b),
        _ => true,
    };
    if same {
        return base.clone();
    }
    let mut out = base.clone();
    out.cookies = None;
    out.auth = None;
    out.extra.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization") && !k.eq_ignore_ascii_case("cookie"));
    out
}

pub fn parse_retry_after(val: &str) -> Option<Duration> {
    val.trim().parse::<u64>().ok().map(|s| Duration::from_secs(s.clamp(1, 60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fake_png_and_id3_preambles_keeping_ts_sync() {
        let mut fake_png = b"\x89PNG\r\n\x1a\nfake-header-bytes".to_vec();
        let mut ts = vec![0u8; 188 * 3];
        ts[0] = 0x47;
        ts[188] = 0x47;
        ts[376] = 0x47;
        fake_png.extend_from_slice(&ts);
        assert_eq!(strip_ts_preamble(&fake_png), ts.as_slice());

        let mut id3 = b"ID3\x04\x00extra-tags".to_vec();
        id3.extend_from_slice(&ts);
        assert_eq!(strip_ts_preamble(&id3), ts.as_slice());
    }

    #[test]
    fn detects_fmp4_and_scopes_cross_origin_headers() {
        assert_eq!(detect_container(b"\x00\x00\x00\x18ftypisomxxxx"), HlsContainer::Fmp4);
        assert_eq!(detect_container(b"\x47\x40\x00\x10restxxxx"), HlsContainer::MpegTs);
        assert_eq!(parse_retry_after(" 12 "), Some(Duration::from_secs(12)));

        let base = RequestHeaders {
            cookies: Some("sid=secret".into()),
            referer: Some("https://site.example/watch".into()),
            extra: vec![("Authorization".into(), "Bearer tok".into()), ("Origin".into(), "https://site.example".into())],
            ..Default::default()
        };
        let same = scoped_headers(&base, "https://cdn.site.example/m.m3u8", "https://edge.site.example/1.ts");
        assert!(same.cookies.is_some());
        let cross = scoped_headers(&base, "https://site.example/m.m3u8", "https://othercdn.net/1.ts");
        assert!(cross.cookies.is_none());
        assert!(!cross.extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")));
        assert!(cross.referer.is_some());
    }
}
