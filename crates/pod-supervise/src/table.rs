//! The container table: running state, in the launcher, persisted to the store.
//!
//! [`TODO/supervise.md`](../../../TODO/supervise.md) T-0604 and T-0601.
//!
//! ⛔ **Running state is launcher state, and it is never inferred from the
//! filesystem.** `TOOL.md` section 6.6 and T-0601: scanning `/proc/*/root/<marker>`
//! races against exit, depends on other processes' root links being readable,
//! and trusts a path any writable payload can create. podvm keeps one table,
//! written by the launcher under the store's own lock.
//!
//! ⛔ **A launcher that was killed leaves the table saying `running`, and the
//! only honest answer after that is "this process exited while podvm was not
//! watching".** T-0604's Decision. [`reconcile`] establishes that by trying to
//! take the container's lock: a launcher holds it for its whole life, so a lock
//! this process can take is a launcher that is gone. ⚠ Never by pid: a pid is
//! reused, and a start-time check beside it is the heuristic T-0601 rejects.

use std::path::PathBuf;

use podvm_image::store::Lock;
use podvm_image::Store;
use serde::{Deserialize, Serialize};

/// ⭐ A version discriminator, as on everything else this project persists.
pub const TABLE_VERSION: u32 = 1;
pub const TABLE_FILE: &str = "containers.json";
pub const CONTAINERS_DIR: &str = "containers";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Made, never started.
    Created,
    /// A launcher holds its lock and its pidfd.
    Running,
    /// It ran and ended, and podvm saw it end.
    Exited,
    /// ⛔ It ran and ended while nothing was watching. Distinct from `Exited`
    /// because the exit CODE is unknown, and reporting a guess as one is the
    /// class of lie this runtime exists to refuse.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    /// The reference as the caller wrote it.
    pub image: String,
    pub manifest_digest: String,
    pub rootfs: String,
    pub argv: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub state: State,
    /// The payload's pid. ⚠ For reporting only: nothing addresses a process by
    /// it, because a pid is reused.
    pub pid: Option<i64>,
    /// The supervisor that holds the pidfd and the lock.
    pub launcher_pid: Option<i64>,
    pub exit_code: Option<i32>,
    /// ⛔ When a later podvm NOTICED it had ended unwatched. T-0604: recording
    /// when it was noticed rather than when it happened, because the second is
    /// not a thing this process knows.
    pub noticed: Option<String>,
    /// The rung the launcher selected when it started this container.
    pub rung: String,
    /// ⭐ TODO/enter.md T-1411. The exact argv the launcher execs on a
    /// detached userland entry: the image's loader, the payload by host
    /// path, then the payload's own arguments (the applet name riding
    /// beside a resolved busybox file where one was inserted). Empty
    /// everywhere else, including chroot records and the memfd family,
    /// which has no detached driver: the launcher enters through
    /// `spawn_selected` there. The libraries, the `PODVM_MAPS` table
    /// and `PODVM_GUEST_EXE` ride in `env`, decided beside this argv at
    /// `create`. `serde(default)` so a table written before this parses.
    #[serde(default)]
    pub userland_exec: Vec<String>,
    /// ⭐ What the completion layer did to this container's rootfs, one line
    /// per fixup that changed a byte or failed.
    /// [`TODO/cli.md`](../../../TODO/cli.md) T-0804 rule 3: `inspect` reports
    /// the true mode PER CONTAINER, and a `/dev/null` that is a regular file is
    /// part of that mode. ⚠ `serde(default)` so a table written before M5 still
    /// parses; an empty list there means "not recorded", which is why
    /// [`Container::completion_note`] says so rather than printing nothing.
    #[serde(default)]
    pub completion: Vec<String>,
    #[serde(default)]
    pub completion_degraded: usize,
}

impl Container {
    pub fn short_id(&self) -> String {
        self.id.chars().take(12).collect()
    }

    /// ⛔ A dash where the record predates M5, never an empty list read as
    /// "nothing was done": the two are different facts and only one of them is
    /// a claim about the rootfs.
    pub fn completion_note(&self) -> String {
        if self.completion.is_empty() {
            "-".to_string()
        } else {
            self.completion.join("; ")
        }
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
            State::Dead => "Dead (exited while podvm was not watching)".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub podvm_containers: u32,
    #[serde(default)]
    pub containers: Vec<Container>,
}

impl Default for Table {
    fn default() -> Table {
        Table {
            podvm_containers: TABLE_VERSION,
            containers: Vec::new(),
        }
    }
}

/// Where one container's own files live: its lock, its log and its control
/// socket. ⚠ Under the store, so the same containment gate covers them.
pub fn dir(store: &Store, id: &str) -> PathBuf {
    store.root().join(CONTAINERS_DIR).join(id)
}

pub fn lock_path(store: &Store, id: &str) -> PathBuf {
    dir(store, id).join("lock")
}

pub fn log_path(store: &Store, id: &str) -> PathBuf {
    dir(store, id).join("log")
}

pub fn control_path(store: &Store, id: &str) -> PathBuf {
    dir(store, id).join("ctl.sock")
}

/// Where one container's ownership memo lives: beside its record, on the host.
///
/// ⛔ T-0710's ruling. The payload cannot reach it; the interposer is handed a
/// descriptor to it at spawn, re-handed on every `exec` re-entry. Nothing on
/// the host reads it to decide anything: it is the payload's own view of
/// ownership, not evidence.
pub fn memo_path(store: &Store, id: &str) -> PathBuf {
    dir(store, id).join("ownership.memo")
}

/// The variable carrying the memo descriptor number into the payload.
///
/// One fact in two homes with `podvm-interpose`'s `MEMO_FD_VAR`: that crate
/// cannot depend on this one, so the two spellings are asserted to agree by
/// driving the behaviour end to end in
/// `experiments/105-interpose-ownership.sh`.
pub const MEMO_FD_VAR: &str = "PODVM_MEMO_FD";

/// The descriptor number the payload sees the memo on.
///
/// High enough to miss 0, 1, 2 and the log sinks the launcher passes, low
/// enough to stay under any conservative `FD_SETSIZE`. The host's own number
/// for the file is unrelated: `podvm-enter`'s `Fds.pass` dups it to this one
/// in the child before the `chroot`, and `dup3` with flags 0 clears
/// `CLOEXEC`, so it survives the `execve` the readiness pipe does not.
pub const MEMO_CHILD_FD: i64 = 17;

/// The `PODVM_MEMO_FD` entry for a payload environment, once the host has the
/// file open.
pub fn memo_fd_env() -> String {
    format!("{MEMO_FD_VAR}={MEMO_CHILD_FD}")
}

/// The memo descriptor the environment hands the payload, or `None`.
///
/// Decimal digits only, within `c_int` range; the interposer parses the same
/// grammar, so anything else is no descriptor rather than a guess at one.
pub fn memo_fd_of(env: &[String]) -> Option<i64> {
    let v = env
        .iter()
        .rev()
        .find_map(|e| e.strip_prefix(&format!("{MEMO_FD_VAR}=")))?;
    if v.is_empty() || v.len() > 10 || !v.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: i64 = v.parse().ok()?;
    (n >= 0 && n <= i32::MAX as i64).then_some(n)
}

mod memo;
pub use memo::*;

/// The control socket's path, in a form that FITS IN `sun_path`.
///
/// ⛔ **A unix socket address is 108 bytes including the NUL, and a container
/// path is not.** Measured on 2026-09-09: a default store plus `containers/`
/// plus a 64-character id plus `/ctl.sock` is 110 bytes, and `bind` refuses it
/// with "path must be shorter than SUN_LEN". Shortening the id would trade a
/// hard limit for a collision nobody would ever diagnose.
///
/// ⭐ So the directory is opened and addressed through `/proc/self/fd/<n>`,
/// which is about 24 bytes whatever the store root is. The returned [`File`]
/// MUST outlive the bind or connect: closing it removes the path.
pub fn control_via_fd(store: &Store, id: &str) -> crate::Result<(std::fs::File, PathBuf)> {
    let d = dir(store, id);
    let f = std::fs::File::open(&d).map_err(|e| crate::Error(format!("{}: {e}", d.display())))?;
    use std::os::fd::AsRawFd;
    let path = PathBuf::from(format!("/proc/self/fd/{}/ctl.sock", f.as_raw_fd()));
    Ok((f, path))
}

fn table_path(store: &Store) -> PathBuf {
    store.root().join(TABLE_FILE)
}

/// Read the table, with no lock. ⛔ Safe because the table is REPLACED by a
/// rename, exactly as the image index is: invariant I2 of
/// `crates/podvm-image/src/store.rs`.
pub fn read(store: &Store) -> crate::Result<Table> {
    match std::fs::read(table_path(store)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            crate::Error(format!(
                "{} does not parse: {e}. It is podvm's own container table",
                table_path(store).display()
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Table::default()),
        Err(e) => Err(crate::Error(format!(
            "{}: {e}",
            table_path(store).display()
        ))),
    }
}

fn write(store: &Store, table: &Table) -> crate::Result<()> {
    let path = table_path(store);
    let bytes = serde_json::to_vec_pretty(table)
        .map_err(|e| crate::Error(format!("serialising the container table: {e}")))?;
    // ⛔ Staged and renamed, never edited in place. Invariant I2 again, and the
    // staging directory is inside the store so the rename cannot be `EXDEV`.
    let tmp = store.root().join("staging").join(format!(
        "{TABLE_FILE}.{}.{}.partial",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    if let Some(parent) = tmp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&tmp, &bytes).map_err(|e| crate::Error(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path).map_err(|e| crate::Error(format!("{}: {e}", path.display())))
}

/// Read the table, hand it to `f`, and write back what `f` leaves.
///
/// ⛔ **Under the store's own lock, invariant I1.** One writer at a time over
/// one table, and the same lock the image index takes, so a `prune` and a
/// container write cannot interleave either.
pub fn update<T>(
    store: &Store,
    f: impl FnOnce(&mut Table) -> crate::Result<T>,
) -> crate::Result<T> {
    let _guard = store
        .lock()
        .map_err(|e| crate::Error(format!("locking the store: {e}")))?;
    let mut table = read(store)?;
    let out = f(&mut table)?;
    write(store, &table)?;
    Ok(out)
}

/// ⛔ T-0604. Mark as `Dead` anything the table calls running whose launcher is
/// gone, and say when it was noticed.
///
/// ⚠ Returns the reconciled table rather than printing: whether a reconciliation
/// deserves a line is the caller's question, and `ps` runs this on every call.
pub fn reconcile(store: &Store) -> crate::Result<Table> {
    update(store, |table| {
        let now = podvm_image::clock::now();
        for c in table.containers.iter_mut() {
            if c.state != State::Running {
                continue;
            }
            let path = lock_path(store, &c.id);
            if !path.exists() {
                c.state = State::Dead;
                c.noticed = Some(now.clone());
                continue;
            }
            // ⭐ The whole test. A launcher holds this for its entire life.
            match Lock::try_exclusive(&path) {
                Ok(Some(_free)) => {
                    c.state = State::Dead;
                    c.noticed = Some(now.clone());
                    c.finished_at.get_or_insert_with(|| now.clone());
                }
                // Held: the launcher is alive and the record is true.
                Ok(None) => {}
                // ⚠ Could not ask. Not an answer, so the record is left alone
                // rather than being changed on the strength of a failed check.
                Err(_) => {}
            }
        }
        Ok(table.clone())
    })
}

/// Resolve a name, a full id or an unambiguous id prefix.
///
/// ⛔ Ambiguity is an error rather than a choice. Picking the first match is how
/// a caller gets the wrong container and never learns it.
pub fn find(table: &Table, want: &str) -> crate::Result<Container> {
    let mut hits: Vec<&Container> = table
        .containers
        .iter()
        .filter(|c| c.name == want || c.id == want)
        .collect();
    if hits.is_empty() {
        hits = table
            .containers
            .iter()
            .filter(|c| c.id.starts_with(want) && !want.is_empty())
            .collect();
    }
    match hits.len() {
        0 => Err(crate::Error(format!(
            "no such container: {want}. `podvm ps -a` lists what there is"
        ))),
        1 => Ok(hits[0].clone()),
        n => Err(crate::Error(format!(
            "{want} names {n} containers: {}. Give a longer prefix or a name",
            hits.iter()
                .map(|c| c.short_id())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests;
