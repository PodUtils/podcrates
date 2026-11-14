//! The block blob calls of the signed upload URLs of the cache, and the
//! download of an entry, whole or by a `Range`.

use std::fs::{self, File};
use std::io::{Seek, SeekFrom};

use super::CacheStore;
use crate::blob::{BLOCK_ID_MAX, block_file, join};
use crate::wire::{Answer, CONTENT_BLOB, Payload, block_list, query, same, text};

/// The part of `len` bytes that a `Range: bytes=A-B` header names: the
/// first and the last byte.
pub(crate) fn range(header: &str, len: u64) -> Option<(u64, u64)> {
    let (a, b) = header.trim().strip_prefix("bytes=")?.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => (len.checked_sub(n.parse().ok()?)?, len.checked_sub(1)?),
        (s, "") => (s.parse().ok()?, len.checked_sub(1)?),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    (start <= end && end < len).then_some((start, end))
}

impl CacheStore {
    /// A block, a block list, or a whole blob of an upload.
    pub(super) fn put(&self, id: u64, sig: &str, q: &str, body: &[u8]) -> Answer {
        let mut st = self.lock();
        let Some(p) = st.pending.get(&id) else { return text(403, Vec::new()) };
        if !same(&p.sig, sig) {
            return text(403, Vec::new());
        }
        let up = self.dir.join(".pending").join(&p.dir);
        let comp = query(q, "comp").unwrap_or_default();
        let target = match comp.as_str() {
            "block" => match query(q, "blockid") {
                Some(b) if !b.is_empty() && b.len() <= BLOCK_ID_MAX => block_file(&up, &b),
                _ => return text(400, "no block id, or a block id longer than 88 bytes"),
            },
            "blocklist" => return join(&up, &block_list(&String::from_utf8_lossy(body))),
            "" => up.join("blob"),
            _ => return text(400, format!("no call comp={comp}")),
        };
        // The uploads that are not finalized hold the limit too: an entry
        // over the limit in blocks never reaches the disk.
        let held: u64 = st.pending.values().map(|p| p.bytes).sum();
        if held.saturating_add(body.len() as u64) > self.limit {
            return text(413, format!("the uploads of the cache are over {} bytes", self.limit));
        }
        match fs::write(target, body) {
            Ok(()) => {
                if let Some(p) = st.pending.get_mut(&id) {
                    p.bytes += body.len() as u64;
                }
                text(201, Vec::new())
            }
            Err(e) => text(500, e.to_string()),
        }
    }

    pub(super) fn download(&self, id: &str, sig: &str, header: Option<&str>) -> Answer {
        let Some(e) = self.entries().into_iter().find(|e| e["id"] == id) else { return text(404, Vec::new()) };
        if !same(e["sig"].as_str().unwrap_or(""), sig) {
            return text(403, Vec::new());
        }
        let Ok(mut f) = File::open(self.dir_of(&e).join("blob")) else { return text(404, Vec::new()) };
        let Ok(len) = f.metadata().map(|m| m.len()) else { return text(404, Vec::new()) };
        let ranges = vec![("Accept-Ranges", "bytes".to_string())];
        let Some(h) = header else {
            return Answer { status: 200, content_type: CONTENT_BLOB, headers: ranges, body: Payload::File(f, len) };
        };
        match range(h, len) {
            Some((a, b)) => {
                if let Err(e) = f.seek(SeekFrom::Start(a)) {
                    return text(500, e.to_string());
                }
                let mut headers = ranges;
                headers.push(("Content-Range", format!("bytes {a}-{b}/{len}")));
                Answer { status: 206, content_type: CONTENT_BLOB, headers, body: Payload::File(f, b - a + 1) }
            }
            None => {
                let mut a = text(416, Vec::new());
                a.headers.push(("Content-Range", format!("bytes */{len}")));
                a
            }
        }
    }
}
