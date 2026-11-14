//! The cache of `actions/cache@v4` and of the cache of the `setup-*`
//! actions: the CacheService of the results API (twirp JSON, cache service
//! v2) that they call at `ACTIONS_RESULTS_URL`, and the blob routes of its
//! signed URLs. The store stays across runs.
//!
//! An entry is a key and a version (the client hashes its paths and its
//! compression into the version), in the scope of a ref. A lookup reads
//! each scope in order (the ref of the run, then the default branch, as
//! GitHub does): the exact key first, then each restore key as a prefix,
//! newest first, and only entries of the same version. An entry of a key
//! and a version is written once in its scope. The store has a limit of
//! bytes: the oldest entry goes first. A download takes a `Range`, as the
//! concurrent download of the client asks.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use pod_core::entropy::{Entropy, OsEntropy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::wire::{Answer, hex, ok, text};

mod blob;
#[cfg(feature = "fuzzing")]
pub(crate) use blob::range;
mod service;
mod sweep;

pub use sweep::PENDING_AGE;

/// The path of each call of the service: `SERVICE` and the method.
pub const SERVICE: &str = "/twirp/github.actions.results.api.v1.CacheService/";
/// The path of each blob of a signed URL: `BLOBS`, `upload/ID` or
/// `download/ID`, and the query `sig=`.
pub const BLOBS: &str = "/_cache/";

/// GitHub keeps 10 GB of cache for each repository.
pub const LIMIT: u64 = 10_000_000_000;

/// An answer in memory: the status, the body and the other headers
/// (podrunner's form of an answer of the cache).
pub type Routed = (u16, Vec<u8>, Vec<(&'static str, String)>);

/// The longest key, as GitHub takes it; the version has the same limit.
const KEY_MAX: usize = 512;

/// An upload that is not finalized.
struct Pending {
    key: String,
    version: String,
    sig: String,
    /// The directory of the upload under `.pending`: random, so that two
    /// stores over one directory never share one.
    dir: String,
    /// The bytes that the client sent.
    bytes: u64,
}

struct Inner {
    next: u64,
    pending: BTreeMap<u64, Pending>,
    entropy: Box<dyn Entropy + Send>,
}

/// The cache store.
///
/// The store keeps each entry in `DIR/entries/HASH/` (`blob` and
/// `entry.json`), where `HASH` is a digest of the scope, the key and the
/// version, so no key is a path. An upload that is not finalized is in
/// `DIR/.pending/`, until a sweep removes it.
pub struct CacheStore {
    dir: PathBuf,
    limit: u64,
    /// The scope of a new entry first, then each scope that a lookup reads.
    scopes: Vec<String>,
    inner: Mutex<Inner>,
}

/// The time in nanoseconds of the Unix epoch.
fn now_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// The order of an entry: newer is higher.
fn order(e: &Value) -> u64 {
    e["order"].as_u64().unwrap_or_else(|| e["created"].as_u64().unwrap_or(0).saturating_mul(1_000_000_000))
}

/// An answer of the service: twirp JSON with `ok: false` and the reason,
/// as the client reads a refusal.
fn refused(message: impl Into<String>) -> Answer {
    ok(json!({ "ok": false, "message": message.into() }))
}

impl CacheStore {
    /// A store in `dir` with a limit of `limit` bytes. The signatures and
    /// the ids of the entries take the random bytes of the system.
    pub fn new(dir: &Path, limit: u64) -> io::Result<CacheStore> {
        CacheStore::with_entropy(dir, limit, OsEntropy)
    }

    /// A store with the random source `entropy`. When the source fails,
    /// `CreateCacheEntry` and the finalize fail: a signature or an id is
    /// never made of weaker bytes. An upload of an earlier run that did not
    /// change for [`PENDING_AGE`] goes ([`CacheStore::sweep`]).
    pub fn with_entropy(dir: &Path, limit: u64, entropy: impl Entropy + Send + 'static) -> io::Result<CacheStore> {
        fs::create_dir_all(dir.join(".pending"))?;
        fs::create_dir_all(dir.join("entries"))?;
        let inner = Inner { next: 0, pending: BTreeMap::new(), entropy: Box::new(entropy) };
        let store = CacheStore { dir: dir.to_path_buf(), limit, scopes: vec![String::new()], inner: Mutex::new(inner) };
        store.sweep(SystemTime::now());
        Ok(store)
    }

    /// The scopes of the run: a new entry goes into the first; a lookup
    /// reads each in order. A scope that is there again counts once; no
    /// scope keeps the scope `""`.
    pub fn with_scopes(mut self, scopes: Vec<String>) -> CacheStore {
        let mut seen = Vec::new();
        for s in scopes {
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        if !seen.is_empty() {
            self.scopes = seen;
        }
        self
    }

    /// The directory of the store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Random bytes in hex, or the reason when the source fails.
    fn random(&self, st: &mut Inner, len: usize) -> Result<String, String> {
        let mut b = vec![0u8; len];
        st.entropy.fill(&mut b).map_err(|e| e.to_string())?;
        Ok(hex(&b))
    }

    /// The directory of an entry: a digest of its scope, key and version,
    /// each as a string of JSON, so that no two of them give one name.
    fn entry_dir_in(&self, scope: &str, key: &str, version: &str) -> PathBuf {
        let d = hex(&Sha256::digest(json!([scope, key, version]).to_string().as_bytes()));
        self.dir.join("entries").join(&d[..32])
    }

    fn entry_dir(&self, key: &str, version: &str) -> PathBuf {
        self.entry_dir_in(&self.scopes[0], key, version)
    }

    /// The directory of an entry that `entries` gave.
    fn dir_of(&self, e: &Value) -> PathBuf {
        let s = |k: &str| e[k].as_str().unwrap_or("").to_string();
        self.entry_dir_in(&s("scope"), &s("key"), &s("version"))
    }

    /// Each entry of the store, newest first, with its signature. A record
    /// counts only in the directory of its own scope, key and version: a
    /// download reads the blob there.
    pub fn entries(&self) -> Vec<Value> {
        let mut out: Vec<Value> = fs::read_dir(self.dir.join("entries"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let v: Value = serde_json::from_str(&fs::read_to_string(e.path().join("entry.json")).ok()?).ok()?;
                (e.path() == self.dir_of(&v)).then_some(v)
            })
            .collect();
        out.sort_by_key(|e: &Value| std::cmp::Reverse((order(e), e["id"].as_str().map(str::to_string))));
        out
    }

    /// The entry for a key and its restore keys, of one version.
    fn lookup(&self, key: &str, restore: &[&str], version: &str) -> Option<Value> {
        let entries = self.entries();
        self.scopes.iter().find_map(|scope| {
            let all: Vec<&Value> = entries
                .iter()
                .filter(|e| e["version"] == version && e["scope"].as_str().unwrap_or("") == scope)
                .collect();
            if let Some(e) = all.iter().find(|e| e["key"] == key) {
                return Some((*e).clone());
            }
            restore.iter().find_map(|p| {
                all.iter().find(|e| e["key"].as_str().is_some_and(|k| k.starts_with(p))).cloned().cloned()
            })
        })
    }

    /// The answer to a request of the service or of a blob, or `None` for
    /// another route. `path` keeps its query; `range` is the value of the
    /// `Range` header of a download. `base` is the URL of the server with a
    /// `/` at the end; the signed URLs start with it.
    pub fn answer(&self, method: &str, path: &str, body: &[u8], range: Option<&str>, base: &str) -> Option<Answer> {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        if let Some(call) = p.strip_prefix(SERVICE) {
            if method != "POST" {
                return Some(text(405, Vec::new()));
            }
            let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            return Some(self.call(call, &v, base));
        }
        let (kind, id) = p.strip_prefix(BLOBS)?.split_once('/')?;
        let sig = crate::wire::query(q, "sig").unwrap_or_default();
        Some(match (method, kind) {
            ("PUT", "upload") => self.put(id.parse().ok()?, &sig, q, body),
            ("GET", "download") => self.download(id, &sig, range),
            _ => text(404, Vec::new()),
        })
    }

    /// [`CacheStore::answer`] with the body in memory, and the other
    /// headers.
    pub fn route(&self, method: &str, path: &str, body: &[u8], range: Option<&str>, base: &str) -> Option<Routed> {
        let mut a = self.answer(method, path, body, range, base)?;
        let headers = std::mem::take(&mut a.headers);
        let (status, bytes) = a.into_bytes();
        Some((status, bytes, headers))
    }

    /// Remove the oldest entries until the store is within its limit.
    fn evict(&self) {
        let mut entries = self.entries();
        let mut total: u64 = entries.iter().filter_map(|e| e["size"].as_u64()).sum();
        while total > self.limit {
            let Some(old) = entries.pop() else { break };
            let _ = fs::remove_dir_all(self.dir_of(&old));
            total = total.saturating_sub(old["size"].as_u64().unwrap_or(0));
        }
    }
}

#[cfg(test)]
mod tests;
