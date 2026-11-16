//! The small helper the browser itself starts for the Aknan Download add-in
//! ("native messaging"). The add-in talks to it through the browser, never
//! over the network, so nothing the browser does about web pages reaching
//! this computer (and no port another program or Windows keeps) can stop it.
//!
//! Each message of the add-in is one request for the program; this helper
//! hands it on to the program on this computer (signed with the program's
//! key, read from the program's own files) and gives the answer back. When
//! the program is closed and the user asked for a download, it opens it.
//!
//! Messages: 4 bytes of length (this computer's order) and then JSON, both ways.
//! A console program, as the browsers expect (they start it out of sight).

mod client;
mod place;
#[cfg(test)]
mod tests;

use place::Place;
use serde_json::Value;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// The browser gives at most this much (Chrome allows 64 MB).
const MAX_IN: usize = 64 << 20;

fn main() {
    let place = Arc::new(Place::find());
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let mut input = std::io::stdin().lock();
    // the browser closes our input when the add-in lets go: then we end
    while let Some(msg) = read_message(&mut input) {
        let (out, place) = (out.clone(), place.clone());
        // a video look-up takes seconds: the other requests don't wait for it
        std::thread::spawn(move || {
            let answer = client::answer(&place, &msg);
            send(&out, &answer);
        });
    }
}

pub(crate) fn read_message(input: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_IN {
        return None;
    }
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    Some(serde_json::from_slice(&buf).unwrap_or(Value::Null))
}

fn send(out: &Mutex<std::io::Stdout>, v: &Value) {
    let data = v.to_string();
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = o.write_all(&(data.len() as u32).to_ne_bytes());
    let _ = o.write_all(data.as_bytes());
    let _ = o.flush();
}
