//! Passwords and cookies on the disk: encrypted with the Windows user's own
//! key (DPAPI, `CryptProtectData`). Only the same Windows user on the same
//! computer can read them back; a settings or list file copied elsewhere (or
//! read by another user) shows no password.
//!
//! On the disk a protected text looks like `"dpapi:<hex>"`. Files of older
//! versions hold plain text: it is read as it is and protected on the next
//! save. Use the field attributes below:
//!
//! ```ignore
//! #[serde(with = "crate::secret::string")]          pub password: String,
//! #[serde(default, with = "crate::secret::opt")]    pub cookies: Option<String>,
//! #[serde(default, with = "crate::secret::pair")]   pub auth: Option<(String, String)>,
//! ```

const PREFIX: &str = "dpapi:";

/// The signature of one request to the program: HMAC-SHA256 with the
/// program's key over "METHOD\npath\ntime\nnonce" (hex). The key itself is
/// never sent; a request seen once can't be sent again (the nonce).
pub fn request_signature(key: &str, method: &str, path: &str, time: &str, nonce: &str) -> String {
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
    let msg = format!("{method}\n{path}\n{time}\n{nonce}");
    to_hex(ring::hmac::sign(&k, msg.as_bytes()).as_ref())
}

/// `sig` (hex) is the signature of this request (checked in constant time).
pub fn signature_ok(key: &str, method: &str, path: &str, time: &str, nonce: &str, sig: &str) -> bool {
    let Some(raw) = from_hex(sig.trim()) else { return false };
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
    let msg = format!("{method}\n{path}\n{time}\n{nonce}");
    ring::hmac::verify(&k, msg.as_bytes(), &raw).is_ok()
}

/// A random nonce (32 hex characters).
pub fn random_nonce() -> String {
    random_token()[..32].to_string()
}

/// A new random secret (64 hex characters), e.g. the key the browser add-in
/// shows the program with every request.
pub fn random_token() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 32];
    if ring::rand::SystemRandom::new().fill(&mut b).is_err() {
        // never seen; still unique per start
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b[..16].copy_from_slice(&(t ^ u128::from(std::process::id())).to_le_bytes());
    }
    to_hex(&b)
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(windows)]
#[path = "win.rs"]
mod os;

#[cfg(not(windows))]
#[path = "fallback.rs"]
mod os;

/// Texts protected already in this run: the download list is saved every
/// few seconds and each protection is a Windows call, so the same cookie
/// text is protected once, not again and again. (Only in memory, where the
/// plain text is anyway.)
static SEALED: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> = std::sync::Mutex::new(None);

/// The text as it is kept on the disk.
pub fn seal(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    let mut cache = SEALED.lock().unwrap_or_else(|p| p.into_inner());
    let map = cache.get_or_insert_with(Default::default);
    if let Some(done) = map.get(plain) {
        return done.clone();
    }
    let sealed = match os::protect(plain.as_bytes()) {
        Some(enc) => format!("{PREFIX}{}", to_hex(&enc)),
        None => plain.to_string(),
    };
    // a cookie text is a few KB: keep at most about a thousand of them
    if map.len() > 1000 {
        map.clear();
    }
    map.insert(plain.to_string(), sealed.clone());
    sealed
}

/// The text from the disk. A protected text that can't be opened (another
/// user or computer) becomes empty: the user types the password again.
pub fn open(stored: &str) -> String {
    match stored.strip_prefix(PREFIX) {
        Some(hex) => from_hex(hex)
            .and_then(|b| os::unprotect(&b))
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default(),
        None => stored.to_string(),
    }
}

pub mod string {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::seal(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(super::open(&String::deserialize(d)?))
    }
}

pub mod opt {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().map(super::seal).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(|x| super::open(&x)).filter(|x| !x.is_empty()))
    }
}

pub mod pair {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<(String, String)>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref().map(|(u, p)| (u.clone(), super::seal(p))).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<(String, String)>, D::Error> {
        Ok(Option::<(String, String)>::deserialize(d)?.map(|(u, p)| (u, super::open(&p))))
    }
}

#[cfg(test)]
mod tests;
