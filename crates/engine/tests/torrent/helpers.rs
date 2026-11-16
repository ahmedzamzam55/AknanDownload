use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::bt::Torrents;
use swiftget_engine::torrent::{parse_torrent, AddOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(crate) fn ben(s: &[u8]) -> Vec<u8> {
    let mut v = format!("{}:", s.len()).into_bytes();
    v.extend_from_slice(s);
    v
}

/// The info part of a .torrent of `data` (single file `name`).
pub(crate) fn make_info(name: &str, data: &[u8]) -> Vec<u8> {
    let piece = 16 * 1024;
    let mut pieces = Vec::new();
    for c in data.chunks(piece) {
        pieces.extend_from_slice(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, c).as_ref());
    }
    let mut info = b"d".to_vec();
    info.extend(b"6:length");
    info.extend(format!("i{}e", data.len()).into_bytes());
    info.extend(b"4:name");
    info.extend(ben(name.as_bytes()));
    info.extend(b"12:piece length");
    info.extend(format!("i{piece}e").into_bytes());
    info.extend(b"6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    info
}

/// A real .torrent of `data` (single file `name`) with this tracker.
pub(crate) fn make_torrent(name: &str, data: &[u8], tracker: &str) -> Vec<u8> {
    let info = make_info(name, data);
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(tracker.as_bytes()));
    t.extend(b"4:info");
    t.extend(info);
    t.extend(b"e");
    t
}

/// The smallest HTTP tracker: everyone who announces gets everyone else.
pub(crate) async fn tracker() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let peers: Arc<Mutex<Vec<u16>>> = Arc::default();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let peers = peers.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let p: u16 = req
                    .split(['?', '&', ' '])
                    .find_map(|kv| kv.strip_prefix("port=").and_then(|v| v.parse().ok()))
                    .unwrap_or(0);
                let list = {
                    let mut ps = peers.lock().unwrap();
                    if p > 0 && !ps.contains(&p) {
                        ps.push(p);
                    }
                    ps.clone()
                };
                let mut compact = Vec::new();
                for q in list.iter().filter(|q| **q != p) {
                    compact.extend_from_slice(&[127, 0, 0, 1]);
                    compact.extend_from_slice(&q.to_be_bytes());
                }
                let mut body = b"d8:intervali2e5:peers".to_vec();
                body.extend(ben(&compact));
                body.extend(b"e");
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    port
}

/// Wait until the torrent `g` of `t` has all its chosen files.
pub(crate) async fn complete(t: &Torrents, g: &str, secs: u64) -> bool {
    for _ in 0..secs * 10 {
        let st = t.status(g).await.unwrap();
        assert_ne!(st.status, "error", "{:?}", st.error);
        if st.seeding && st.total > 0 && st.done == st.total {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// A folder, a file, its .torrent, and a first session that shares it.
pub(crate) async fn shared_file(tag: &str) -> Option<(std::path::PathBuf, Vec<u8>, std::path::PathBuf, Arc<Torrents>, String, u16)> {
    let root = std::env::temp_dir().join(format!("aknan-torrent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seed_dir = root.join("seed");
    std::fs::create_dir_all(&seed_dir).unwrap();
    // different bytes for every test: different torrents, no mix-ups
    let salt = tag.bytes().map(u32::from).sum::<u32>();
    let data: Vec<u8> = (0..300_000u32).map(|i| ((i * 7 + salt) % 253) as u8).collect();
    std::fs::write(seed_dir.join("song.bin"), &data).unwrap();
    let port = tracker().await;
    let file = root.join("song.torrent");
    std::fs::write(&file, make_torrent("song.bin", &data, &format!("http://127.0.0.1:{port}/announce"))).unwrap();
    let info = parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.name, "song.bin");
    assert_eq!(info.total, data.len() as u64);

    // the computer that has it: checks its copy, then shares
    let seeder = Arc::new(Torrents::new(&root.join("a")));
    let opts = AddOptions { dir: seed_dir.clone(), ..Default::default() };
    let sg = seeder.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await, "the first session should share the file");
    Some((root, data, file, seeder, sg, port))
}

/// A torrent of two files ("pack/a.bin", "pack/b.bin").
pub(crate) fn make_multi(a: &[u8], b: &[u8], tracker: &str) -> (Vec<u8>, Vec<u8>) {
    let piece = 16 * 1024;
    let all: Vec<u8> = a.iter().chain(b).copied().collect();
    let mut pieces = Vec::new();
    for c in all.chunks(piece) {
        pieces.extend_from_slice(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, c).as_ref());
    }
    let file = |len: usize, name: &str| {
        let mut f = b"d6:length".to_vec();
        f.extend(format!("i{len}e").into_bytes());
        f.extend(b"4:pathl");
        f.extend(ben(name.as_bytes()));
        f.extend(b"ee");
        f
    };
    let mut info = b"d5:filesl".to_vec();
    info.extend(file(a.len(), "a.bin"));
    info.extend(file(b.len(), "b.bin"));
    info.extend(b"e4:name");
    info.extend(ben(b"pack"));
    info.extend(b"12:piece lengthi16384e6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(tracker.as_bytes()));
    t.extend(b"4:info");
    t.extend(info.clone());
    t.extend(b"e");
    (t, info)
}
