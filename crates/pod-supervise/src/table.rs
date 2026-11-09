//! The table of containers: the state of the launchers, kept in a file.
//!
//! The running state is the state of the launcher, and it never comes from
//! the file system. A scan of `/proc/*/root/MARKER` races against the exit,
//! needs the root links of other processes, and trusts a path that each
//! payload that can write can make. The crate keeps one table, which the
//! launcher writes under the lock of the table.
//!
//! A launcher that was killed leaves the table at `running`, and the only
//! true answer after that is "this payload ended while nothing watched it".
//! [`reconcile`] finds that when it can take the lock of the container: a
//! launcher holds it for its whole life, so a lock that this process can
//! take is a launcher that is gone. Never by pid: a pid is used again, and a
//! check of the start time beside it is a guess.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use pod_core::{Class, Failure};
use serde::de::DeserializeOwned;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::lock::Lock;
use crate::{Result, Settings, failure, io_at};

/// The version of the table file, under the key of
/// [`Settings::table_key`].
pub const TABLE_VERSION: u32 = 1;
pub const TABLE_FILE: &str = "containers.json";
pub const CONTAINERS_DIR: &str = "containers";
/// The directory of the new table before its rename, under the root: in the
/// same file system, so that the rename cannot fail with `EXDEV`.
pub const STAGING_DIR: &str = "staging";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Made, never started.
    Created,
    /// A launcher holds its lock and its pidfd.
    Running,
    /// It ran and ended, and a launcher saw the end.
    Exited,
    /// It ran and ended while nothing watched it. Apart from `Exited`,
    /// because the exit code is not known, and a guess written as one is a
    /// false record.
    Dead,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Created => "created",
            State::Running => "running",
            State::Exited => "exited",
            State::Dead => "dead",
        }
    }
}

/// The record of one container. `of` holds the fields of the caller; they
/// are in the same JSON object as the fields of the crate, and `Deref`
/// gives them as fields of the record (`c.image` for `c.of.image`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container<X> {
    pub id: String,
    pub name: String,
    pub argv: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub state: State,
    /// The pid of the payload. For a report only: nothing addresses a
    /// process by it, because a pid is used again.
    pub pid: Option<i64>,
    /// The launcher that holds the pidfd and the lock.
    pub launcher_pid: Option<i64>,
    pub exit_code: Option<i32>,
    /// When a later call noticed that the payload ended with no watcher.
    /// The time of the notice, not of the end: this process does not know
    /// the second.
    pub noticed: Option<String>,
    #[serde(flatten)]
    pub of: X,
}

impl<X> Deref for Container<X> {
    type Target = X;
    fn deref(&self) -> &X {
        &self.of
    }
}

impl<X> DerefMut for Container<X> {
    fn deref_mut(&mut self) -> &mut X {
        &mut self.of
    }
}

impl<X> Container<X> {
    pub fn short_id(&self) -> String {
        self.id.chars().take(12).collect()
    }

    /// docker's STATUS column.
    pub fn status(&self) -> String {
        match self.state {
            State::Created => "Created".to_string(),
            State::Running => "Up".to_string(),
            State::Exited => match self.exit_code {
                Some(c) => format!("Exited ({c})"),
                None => "Exited".to_string(),
            },
            State::Dead => "Dead (exited while the launcher was not watching)".to_string(),
        }
    }
}

/// The fields of a caller as JSON values: each key stays as it is, so that
/// a tool that does not know the fields of the owner of a table (a report,
/// a cleanup) writes them back with no change. A type `X` of fewer fields
/// drops the others at each write.
pub type Any = serde_json::Map<String, serde_json::Value>;

/// The table: its version and its records.
#[derive(Debug, Clone)]
pub struct Table<X> {
    pub version: u32,
    pub containers: Vec<Container<X>>,
}

impl<X> Default for Table<X> {
    fn default() -> Table<X> {
        Table { version: TABLE_VERSION, containers: Vec::new() }
    }
}

/// The directory of the files of one container: its lock, its log and its
/// control socket, and the files of the caller.
pub fn dir(s: &Settings, id: &str) -> PathBuf {
    s.root.join(CONTAINERS_DIR).join(id)
}

pub fn lock_path(s: &Settings, id: &str) -> PathBuf {
    dir(s, id).join("lock")
}

pub fn log_path(s: &Settings, id: &str) -> PathBuf {
    dir(s, id).join("log")
}

pub fn control_path(s: &Settings, id: &str) -> PathBuf {
    dir(s, id).join("ctl.sock")
}

/// The directory of a container, when it exists and is inside the
/// directory of the containers: a crafted id cannot reach out of it.
pub fn contained(s: &Settings, id: &str) -> Option<PathBuf> {
    let base = s.root.join(CONTAINERS_DIR).canonicalize().ok()?;
    let real = dir(s, id).canonicalize().ok()?;
    (real != base && real.starts_with(&base)).then_some(real)
}

/// The path of the control socket in a form that fits in `sun_path`.
///
/// A Unix socket address is 108 bytes with the NUL, and the path of a
/// container is not: a root, `containers/`, an id of 64 characters and
/// `/ctl.sock` can be longer, and `bind` refuses it. A shorter id would
/// make a collision that no one could find. So the directory is opened and
/// addressed through `/proc/self/fd/N`, about 24 bytes for each root. The
/// [`std::fs::File`] must live until after the bind or the connect: its
/// close removes the path.
pub fn control_via_fd(s: &Settings, id: &str) -> Result<(std::fs::File, PathBuf)> {
    let d = dir(s, id);
    let f = std::fs::File::open(&d).map_err(|e| io_at(&d, e))?;
    use std::os::fd::AsRawFd;
    let path = PathBuf::from(format!("/proc/self/fd/{}/ctl.sock", f.as_raw_fd()));
    Ok((f, path))
}

fn table_path(s: &Settings) -> PathBuf {
    s.root.join(TABLE_FILE)
}

/// The table in the bytes of its file.
pub fn decode<X: DeserializeOwned>(s: &Settings, bytes: &[u8]) -> Result<Table<X>> {
    let bad = |what: String| {
        failure(
            Class::Protocol,
            format!("{} does not parse: {what}. It is the container table of this tool", table_path(s).display()),
        )
    };
    let mut map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
    let version = match map.get(&s.table_key).and_then(serde_json::Value::as_u64) {
        Some(v) => u32::try_from(v).map_err(|_| bad(format!("the version {v} is too large")))?,
        None => return Err(bad(format!("no version under {:?}", s.table_key))),
    };
    let containers = match map.remove("containers") {
        Some(v) => serde_json::from_value(v).map_err(|e| bad(e.to_string()))?,
        None => Vec::new(),
    };
    Ok(Table { version, containers })
}

/// The bytes of the file of a table: the version under its key first, then
/// the records.
pub fn encode<X: Serialize>(s: &Settings, table: &Table<X>) -> Result<Vec<u8>> {
    struct File<'a, X> {
        key: &'a str,
        table: &'a Table<X>,
    }
    impl<X: Serialize> Serialize for File<'_, X> {
        fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
            let mut m = ser.serialize_map(Some(2))?;
            m.serialize_entry(self.key, &self.table.version)?;
            m.serialize_entry("containers", &self.table.containers)?;
            m.end()
        }
    }
    serde_json::to_vec_pretty(&File { key: &s.table_key, table })
        .map_err(|e| failure(Class::Protocol, format!("writing the container table: {e}")))
}

/// Read the table, with no lock. Safe, because a write replaces the table
/// with a rename: a reader sees the old file or the new one.
pub fn read<X: DeserializeOwned>(s: &Settings) -> Result<Table<X>> {
    let path = table_path(s);
    match std::fs::read(&path) {
        Ok(bytes) => decode(s, &bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Table::default()),
        Err(e) => Err(io_at(&path, e)),
    }
}

fn write<X: Serialize>(s: &Settings, table: &Table<X>) -> Result<()> {
    let path = table_path(s);
    let bytes = encode(s, table)?;
    // A new file and a rename, never a change in place.
    let tmp = s.root.join(STAGING_DIR).join(format!(
        "{TABLE_FILE}.{}.{}.partial",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0)
    ));
    if let Some(parent) = tmp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&tmp, &bytes).map_err(|e| io_at(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io_at(&path, e))
}

/// Read the table, give it to `f`, and write what `f` leaves.
///
/// Under the lock of [`Settings::table_lock`]: one writer at a time.
pub fn update<X, T, E>(
    s: &Settings,
    f: impl FnOnce(&mut Table<X>) -> std::result::Result<T, E>,
) -> std::result::Result<T, E>
where
    X: Serialize + DeserializeOwned,
    E: From<Failure>,
{
    let _guard = Lock::exclusive(&s.table_lock)?;
    let mut table = read(s)?;
    let out = f(&mut table)?;
    write(s, &table)?;
    Ok(out)
}

/// The time of a record, as RFC 3339 UTC.
pub fn now() -> String {
    pod_core::clock::rfc3339(std::time::SystemTime::now())
}

/// Mark `Dead` each record that the table calls running and whose launcher
/// is gone, with the time of the notice. The reconciled table comes back,
/// with no output: whether it deserves a line is the question of the
/// caller.
pub fn reconcile<X>(s: &Settings) -> Result<Table<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    update(s, |table: &mut Table<X>| {
        let now = now();
        for c in table.containers.iter_mut() {
            if c.state != State::Running {
                continue;
            }
            let path = lock_path(s, &c.id);
            if !path.exists() {
                c.state = State::Dead;
                c.noticed = Some(now.clone());
                continue;
            }
            // The whole test: a launcher holds this lock for its whole life.
            match Lock::try_exclusive(&path) {
                Ok(Some(_free)) => {
                    c.state = State::Dead;
                    c.noticed = Some(now.clone());
                    c.finished_at.get_or_insert_with(|| now.clone());
                }
                // Held: the launcher is alive and the record is true.
                Ok(None) => {}
                // The question failed. That is no answer, so the record
                // stays as it is.
                Err(_) => {}
            }
        }
        Ok::<_, Failure>(table.clone())
    })
}

/// The record that `want` names: a name, a full id, or a prefix of one id.
///
/// More than one match is an error, not a choice: a caller that gets the
/// first match can get the wrong container and never know it.
pub fn find<X: Clone>(s: &Settings, table: &Table<X>, want: &str) -> Result<Container<X>> {
    let mut hits: Vec<&Container<X>> = table.containers.iter().filter(|c| c.name == want || c.id == want).collect();
    if hits.is_empty() {
        hits = table.containers.iter().filter(|c| c.id.starts_with(want) && !want.is_empty()).collect();
    }
    match hits.len() {
        0 => Err(failure(
            Class::NotFound,
            match &s.list_command {
                Some(cmd) => format!("no such container: {want}. `{cmd}` lists what there is"),
                None => format!("no such container: {want}"),
            },
        )),
        1 => Ok(hits[0].clone()),
        n => Err(failure(
            Class::Usage,
            format!(
                "{want} names {n} containers: {}. Give a longer prefix or a name",
                hits.iter().map(|c| c.short_id()).collect::<Vec<_>>().join(", ")
            ),
        )),
    }
}

#[cfg(test)]
mod tests;
