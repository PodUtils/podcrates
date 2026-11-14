//! The block blob calls of the signed URLs, as the Azure client of
//! `actions/upload-artifact@v4` sends them, and the download of a zip.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use crate::store::{ArtifactStore, upload_dir};
use crate::wire::{Answer, CONTENT_BLOB, Payload, block_list, hex, query, same, text};

/// The longest block id, in bytes: Azure takes at most 64 bytes before the
/// base64, and 88 after it. The file of a block has its id in hex.
const BLOCK_ID_MAX: usize = 88;

/// The file of a block in the upload directory `up`.
fn block_file(up: &Path, block: &str) -> std::path::PathBuf {
    up.join("blocks").join(hex(block.as_bytes()))
}

/// Join the blocks of `ids` into the blob of the upload `up`, in parts: a
/// blob can be larger than the memory.
fn join(up: &Path, ids: &[String]) -> Answer {
    let part = up.join("blob.part");
    let mut out = match File::create(&part) {
        Ok(f) => f,
        Err(e) => return text(500, e.to_string()),
    };
    for block in ids {
        let Ok(mut f) = File::open(block_file(up, block)) else { return text(400, format!("no block {block}")) };
        if let Err(e) = io::copy(&mut f, &mut out) {
            return text(500, e.to_string());
        }
    }
    drop(out);
    match fs::rename(part, up.join("blob")) {
        Ok(()) => text(201, Vec::new()),
        Err(e) => text(500, e.to_string()),
    }
}

impl ArtifactStore {
    /// A block, a block list, or a whole blob of an upload.
    pub(crate) fn put(&self, id: u64, sig: &str, q: &str, body: &[u8]) -> Answer {
        let mut st = self.lock();
        let Some(p) = st.pending.get(&id) else { return text(403, Vec::new()) };
        if !same(&p.sig, sig) {
            return text(403, Vec::new());
        }
        let Some(at) = self.at(&p.name) else { return text(403, Vec::new()) };
        let up = upload_dir(&at, id);
        let comp = query(q, "comp").unwrap_or_default();
        let target = match comp.as_str() {
            "block" => match query(q, "blockid") {
                Some(b) if !b.is_empty() && b.len() <= BLOCK_ID_MAX => block_file(&up, &b),
                _ => return text(400, "no block id, or a block id longer than 88 bytes"),
            },
            "blocklist" => {
                return join(&up, &block_list(&String::from_utf8_lossy(body)));
            }
            "" => up.join("blob"),
            _ => return text(400, format!("no call comp={comp}")),
        };
        // The limit counts each byte that a client sends in a block or a
        // whole blob; the join of a block list sends none.
        if st.used + body.len() as u64 > self.limit {
            return text(413, format!("the artifacts of the run are over {} bytes", self.limit));
        }
        match fs::write(target, body) {
            Ok(()) => {
                st.used += body.len() as u64;
                text(201, Vec::new())
            }
            Err(e) => text(500, e.to_string()),
        }
    }

    pub(crate) fn download(&self, id: u64, sig: &str) -> Answer {
        let id = id.to_string();
        let Some(m) = self.metas().into_iter().find(|m| m["database_id"] == id.as_str()) else {
            return text(404, Vec::new());
        };
        if !same(m["sig"].as_str().unwrap_or(""), sig) {
            return text(403, Vec::new());
        }
        let Some(at) = m["name"].as_str().and_then(|n| self.at(n)) else { return text(404, Vec::new()) };
        let file = File::open(at.join("artifact.zip")).and_then(|f| Ok((f.metadata()?.len(), f)));
        match file {
            Ok((len, f)) => Answer { status: 200, content_type: CONTENT_BLOB, body: Payload::File(f, len) },
            Err(_) => text(404, Vec::new()),
        }
    }
}
