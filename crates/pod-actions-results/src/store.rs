//! The store of one run: the artifacts on disk, the uploads that are not
//! finalized, the bytes of the run, and the route of a request.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use pod_core::entropy::{Entropy, OsEntropy};
use serde_json::Value;

use crate::wire::{Answer, query, text, valid};
use crate::{BLOBS, SERVICE};

/// An upload that is not finalized.
pub(crate) struct Pending {
    pub(crate) name: String,
    pub(crate) sig: String,
}

pub(crate) struct Inner {
    pub(crate) next: u64,
    pub(crate) pending: BTreeMap<u64, Pending>,
    /// The bytes of the run: each block and each whole blob.
    pub(crate) used: u64,
    pub(crate) entropy: Box<dyn Entropy + Send>,
}

/// The artifact store of one run.
///
/// The store keeps `DIR/NAME/artifact.zip` (the zip of the client, as
/// GitHub gives it) and `DIR/NAME/artifact.json`; an upload that is not
/// finalized is in `DIR/NAME/.upload-ID/`. A name is one component of a
/// path, so no request reaches a file outside `DIR`.
pub struct ArtifactStore {
    pub(crate) dir: PathBuf,
    pub(crate) limit: u64,
    pub(crate) inner: Mutex<Inner>,
}

impl ArtifactStore {
    /// A store in `dir` with a limit of `limit` bytes for the run. The
    /// signatures take the random bytes of the system.
    pub fn new(dir: &Path, limit: u64) -> io::Result<ArtifactStore> {
        ArtifactStore::with_entropy(dir, limit, OsEntropy)
    }

    /// A store with the random source `entropy` for its signatures. When
    /// the source fails, `CreateArtifact` fails: a signature is never made
    /// of weaker bytes.
    pub fn with_entropy(dir: &Path, limit: u64, entropy: impl Entropy + Send + 'static) -> io::Result<ArtifactStore> {
        fs::create_dir_all(dir)?;
        let inner = Inner { next: 0, pending: BTreeMap::new(), used: 0, entropy: Box::new(entropy) };
        let mut store = ArtifactStore { dir: dir.to_path_buf(), limit, inner: Mutex::new(inner) };
        // A store opened again over the artifacts of an earlier run gives
        // each new upload an id above theirs: an id names one artifact.
        let top = store.metas().iter().filter_map(|m| m["database_id"].as_str()?.parse::<u64>().ok()).max();
        store.inner.get_mut().unwrap_or_else(PoisonError::into_inner).next = top.unwrap_or(0);
        Ok(store)
    }

    /// The directory of the store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The bytes that the clients of the run sent, against the limit.
    pub fn used(&self) -> u64 {
        self.lock().used
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The directory of the artifact `name`, or `None` for a name that is
    /// not valid. A valid name is one normal component of a path: never a
    /// parent, a root or a prefix.
    pub(crate) fn at(&self, name: &str) -> Option<PathBuf> {
        if !valid(name) {
            return None;
        }
        let mut parts = Path::new(name).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(_)), None) => Some(self.dir.join(name)),
            _ => None,
        }
    }

    /// The metadata of the finalized artifact `name`, with its signature.
    /// The record must name `name` itself: on a file system that folds
    /// case, `Data` is not the artifact `data`.
    pub(crate) fn meta(&self, name: &str) -> Option<Value> {
        let text = fs::read_to_string(self.at(name)?.join("artifact.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        (v["name"] == name).then_some(v)
    }

    /// Each artifact of the run with its signature, by name.
    pub(crate) fn metas(&self) -> Vec<Value> {
        let mut out: Vec<Value> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.meta(e.file_name().to_str()?))
            .collect();
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    /// Each artifact of the run, by name, as `ListArtifacts` gives it: the
    /// signature of the download stays in the store.
    pub fn list(&self) -> Vec<Value> {
        let mut out = self.metas();
        for a in &mut out {
            if let Some(o) = a.as_object_mut() {
                o.remove("sig");
            }
        }
        out
    }

    /// The answer to a request of the service or of a blob, or `None` for
    /// another route. `path` keeps its query. `base` is the URL of the
    /// server with a `/` at the end; the signed URLs start with it.
    pub fn answer(&self, method: &str, path: &str, body: &[u8], base: &str) -> Option<Answer> {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        if let Some(call) = p.strip_prefix(SERVICE) {
            if method != "POST" {
                return Some(text(405, Vec::new()));
            }
            let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            return Some(self.call(call, &v, base));
        }
        let rest = p.strip_prefix(BLOBS)?;
        let (kind, id) = rest.split_once('/')?;
        let id: u64 = id.parse().ok()?;
        let sig = query(q, "sig").unwrap_or_default();
        Some(match (method, kind) {
            ("PUT", "upload") => self.put(id, &sig, q, body),
            ("GET", "download") => self.download(id, &sig),
            _ => text(404, Vec::new()),
        })
    }

    /// [`ArtifactStore::answer`] with the body in memory.
    pub fn route(&self, method: &str, path: &str, body: &[u8], base: &str) -> Option<(u16, Vec<u8>)> {
        self.answer(method, path, body, base).map(Answer::into_bytes)
    }
}

/// The directory of the upload `id` of the artifact in `at`.
pub(crate) fn upload_dir(at: &Path, id: u64) -> PathBuf {
    at.join(format!(".upload-{id}"))
}
