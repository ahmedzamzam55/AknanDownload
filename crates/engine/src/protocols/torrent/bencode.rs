use anyhow::{anyhow, bail};

/// A value of a .torrent file (bencode).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Ben {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Ben>),
    Dict(Vec<(Vec<u8>, Ben)>),
}

impl Ben {
    pub(super) fn get(&self, key: &str) -> Option<&Ben> {
        match self {
            Ben::Dict(d) => d.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v),
            _ => None,
        }
    }
    pub(super) fn int(&self) -> Option<i64> {
        match self {
            Ben::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub(super) fn text(&self) -> Option<String> {
        match self {
            Ben::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }
    pub(super) fn list(&self) -> &[Ben] {
        match self {
            Ben::List(l) => l,
            _ => &[],
        }
    }
}

pub(super) struct Reader<'a> {
    pub(super) data: &'a [u8],
    pub(super) pos: usize,
    pub(super) depth: u32,
}

impl Reader<'_> {
    pub(super) fn peek(&self) -> anyhow::Result<u8> {
        self.data.get(self.pos).copied().ok_or_else(|| anyhow!("the torrent file is cut short"))
    }
    pub(super) fn number_until(&mut self, end: u8) -> anyhow::Result<i64> {
        let start = self.pos;
        while self.peek()? != end {
            self.pos += 1;
            if self.pos - start > 20 {
                bail!("not a torrent file");
            }
        }
        let s = std::str::from_utf8(&self.data[start..self.pos]).map_err(|_| anyhow!("not a torrent file"))?;
        self.pos += 1;
        s.parse().map_err(|_| anyhow!("not a torrent file"))
    }
    pub(super) fn value(&mut self) -> anyhow::Result<Ben> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("not a torrent file");
        }
        let v = match self.peek()? {
            b'i' => {
                self.pos += 1;
                Ben::Int(self.number_until(b'e')?)
            }
            b'l' => {
                self.pos += 1;
                let mut list = Vec::new();
                while self.peek()? != b'e' {
                    list.push(self.value()?);
                }
                self.pos += 1;
                Ben::List(list)
            }
            b'd' => {
                self.pos += 1;
                let mut dict = Vec::new();
                while self.peek()? != b'e' {
                    let Ben::Bytes(k) = self.value()? else { bail!("not a torrent file") };
                    let v = self.value()?;
                    dict.push((k, v));
                }
                self.pos += 1;
                Ben::Dict(dict)
            }
            b'0'..=b'9' => {
                let len = usize::try_from(self.number_until(b':')?).map_err(|_| anyhow!("not a torrent file"))?;
                let end = self.pos.checked_add(len).filter(|e| *e <= self.data.len()).ok_or_else(|| anyhow!("the torrent file is cut short"))?;
                let b = self.data[self.pos..end].to_vec();
                self.pos = end;
                Ben::Bytes(b)
            }
            _ => bail!("not a torrent file"),
        };
        self.depth -= 1;
        Ok(v)
    }
}

/// Where the "info" part of a .torrent file is (its bytes make the id).
pub(super) fn info_span(data: &[u8]) -> Option<(usize, usize)> {
    let mut r = Reader { data, pos: 0, depth: 0 };
    if r.peek().ok()? != b'd' {
        return None;
    }
    r.pos += 1;
    while r.peek().ok()? != b'e' {
        let Ben::Bytes(k) = r.value().ok()? else { return None };
        let start = r.pos;
        r.value().ok()?;
        if k == b"info" {
            return Some((start, r.pos));
        }
    }
    None
}
