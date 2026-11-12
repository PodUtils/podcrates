//! The private files that the relay client keeps between runs: a minted
//! relay token, the relay's pool of hosts, the rank, the addresses that
//! carried a session, the offers of each host. The files are
//! `pod_core::files`'s: each is read only when it is a regular file of this
//! user that nobody else can read, and is written through a private
//! temporary file and a rename. Nothing here ever prints a token.
//!
//! The places are the caller's ([`Cache`]): the directories of its
//! `pod_core::files::Places`, and what its variable of the cache says
//! (`none`: no file is kept).

use std::path::{Path, PathBuf};

pub use pod_core::files::{
    Chosen, MAX_FILE, Others, Places, Stored, closed, create_new_private, load_file_from, names_from, private_dir,
    read_own, read_private, read_private_up_to, read_trusted, remove_named_from, safe_name, store_file_in_first,
    store_noting,
};
use serde::{Deserialize, Serialize};

/// A cached token is reused only while it has at least this long left.
pub const MIN_REMAINING_MS: i64 = 10 * 60 * 1000;

/// The places of the files of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    /// The directories, in order, and the places passed over.
    pub places: Places,
    /// What the variable of the cache says.
    pub chosen: Chosen,
}

impl Cache {
    /// The cache of `places`, with what its variable says.
    pub fn new(places: Places, chosen: Chosen) -> Cache {
        Cache { places, chosen }
    }

    /// A cache in the directories `dirs` alone, in order (a test's).
    pub fn in_dirs(dirs: Vec<PathBuf>) -> Cache {
        let chosen = if dirs.is_empty() { Chosen::Nothing } else { Chosen::Unset };
        Cache { places: Places { dirs, ..Places::default() }, chosen }
    }

    /// No file at all: each run mints its token and keeps no pool.
    pub fn none() -> Cache {
        Cache::in_dirs(Vec::new())
    }

    /// The directories that a new file is tried in, in order.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.places.dirs
    }

    /// Where a kept file is looked for: the directories, then each place
    /// passed over.
    pub fn read_dirs(&self) -> Vec<PathBuf> {
        self.places.read()
    }

    /// The contents of the private file `name`, from the first directory
    /// that has a trusted one.
    pub fn load_file(&self, name: &str) -> Option<String> {
        load_file_from(self.dirs(), name)
    }

    /// Save `body` as the private file `name` in the first directory that
    /// accepts it.
    pub fn store_file(&self, name: &str, body: &[u8]) -> Result<PathBuf, String> {
        store_file_in_first(self.dirs(), name, body)
    }

    /// The cached token for `relay_host` with at least [`MIN_REMAINING_MS`]
    /// left at `now_ms`, from the first directory that has one, a place
    /// passed over last.
    pub fn load(&self, relay_host: &str, now_ms: i64) -> Option<Cached> {
        load_from(&self.read_dirs(), relay_host, now_ms)
    }

    /// What the cache holds for `relay_host`, without the token.
    pub fn peek(&self, relay_host: &str) -> Option<Peek> {
        peek_from(&self.read_dirs(), relay_host)
    }

    /// Save a token under `key` (see `token::token_key`), with the relay
    /// that minted it, with the refusals of the directories that came first.
    pub fn store_noted(&self, key: &str, token: &str, expires_ms: i64, minted_at: &str) -> Result<Stored, String> {
        let body = entry_body(token, expires_ms, minted_at)?;
        store_noting(self.dirs(), &file_name(key), &body)
    }

    /// Forget the cached token for `relay_host` everywhere it is ours to
    /// delete, a place passed over too, so that a refused token is not read
    /// again there.
    pub fn remove(&self, relay_host: &str) {
        remove_from(&self.read_dirs(), relay_host);
    }

    /// What to say of the variable of the cache, named `dir_env`, once a
    /// file went to `stored`: a relative path that was not used, or the
    /// named directory's refusal. Nothing when the choice was followed.
    pub fn chosen_note(&self, dir_env: &str, stored: &Stored) -> Option<String> {
        pod_core::files::chosen_note(dir_env, &self.chosen, stored)
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    token: String,
    /// Expiry in milliseconds since the epoch, as the relay reports it.
    expires: i64,
    /// The relay that minted the token: `host`, or `host:port`. An entry
    /// written before the minting relay was recorded has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minted_at: Option<String>,
}

/// A token found in the cache. Its `Debug` never shows the token.
pub struct Cached {
    pub token: String,
    pub expires_ms: i64,
    pub path: PathBuf,
    /// The relay that minted it, when the entry says so.
    pub minted_at: Option<String>,
}

impl std::fmt::Debug for Cached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cached")
            .field("token", &"<redacted>")
            .field("expires_ms", &self.expires_ms)
            .field("path", &self.path)
            .field("minted_at", &self.minted_at)
            .finish()
    }
}

/// Whether a string has the shape of a relay token: 8 to 4096 printable ASCII
/// characters and no whitespace (it goes into an HTTP header).
pub fn valid_token(token: &str) -> bool {
    (8..=4096).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

/// The cached token for `relay_host` over explicit directories.
pub fn load_from(dirs: &[PathBuf], relay_host: &str, now_ms: i64) -> Option<Cached> {
    let name = file_name(relay_host);
    dirs.iter().find_map(|dir| {
        let path = dir.join(&name);
        let entry: Entry = serde_json::from_str(&read_trusted(&path)?).ok()?;
        (entry.expires.saturating_sub(now_ms) >= MIN_REMAINING_MS && valid_token(&entry.token)).then_some(Cached {
            token: entry.token,
            expires_ms: entry.expires,
            path,
            minted_at: entry.minted_at,
        })
    })
}

/// What the cache holds for a relay host, without the token: when it
/// expires, the file, and the relay that minted it. The token field is never
/// read into memory, so a status cannot hold or show a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peek {
    pub expires_ms: i64,
    pub path: PathBuf,
    pub minted_at: Option<String>,
}

/// An entry with each field but the token.
#[derive(Deserialize)]
struct Meta {
    expires: i64,
    #[serde(default)]
    minted_at: Option<String>,
}

/// [`Cache::peek`] over explicit directories.
pub fn peek_from(dirs: &[PathBuf], relay_host: &str) -> Option<Peek> {
    let name = file_name(relay_host);
    dirs.iter().find_map(|dir| {
        let path = dir.join(&name);
        let meta: Meta = serde_json::from_str(&read_trusted(&path)?).ok()?;
        Some(Peek { expires_ms: meta.expires, path, minted_at: meta.minted_at })
    })
}

/// The body of an entry, or the refusal of what is no relay token.
fn entry_body(token: &str, expires_ms: i64, minted_at: &str) -> Result<Vec<u8>, String> {
    if !valid_token(token) {
        return Err("refusing to cache something that is not a relay token".into());
    }
    let entry = Entry { token: token.to_string(), expires: expires_ms, minted_at: Some(minted_at.to_string()) };
    serde_json::to_vec(&entry).map_err(|e| e.to_string())
}

/// Save a token under `key` in the first of explicit directories.
pub fn store_in_first(
    dirs: &[PathBuf],
    key: &str,
    token: &str,
    expires_ms: i64,
    minted_at: &str,
) -> Result<PathBuf, String> {
    store_file_in_first(dirs, &file_name(key), &entry_body(token, expires_ms, minted_at)?)
}

/// [`Cache::remove`] over explicit directories.
pub fn remove_from(dirs: &[PathBuf], relay_host: &str) {
    remove_named_from(dirs, &file_name(relay_host));
}

/// `relay-token-<host>.json`, with anything but `[a-z0-9.-]` replaced.
pub fn file_name(relay_host: &str) -> String {
    format!("relay-token-{}.json", pod_core::files::exact_name(relay_host))
}

/// Whether `path` is a file that the cache would read.
pub fn trusted(path: &Path) -> bool {
    read_trusted(path).is_some()
}
