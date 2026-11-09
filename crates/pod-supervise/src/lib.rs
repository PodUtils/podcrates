//! `TOOL.md` section 6.6: pidfd, waitid, logs, the container lifecycle.
//!
//! Milestone M4, [`TODO/milestones.md`](../../../TODO/milestones.md) T-1105 and
//! [`TODO/supervise.md`](../../../TODO/supervise.md) T-0601 to T-0607.
//!
//! ⭐ **Running state is launcher state.** [`table`] is the record and
//! [`launcher`] is the only thing that writes `running` into it. Nothing infers
//! membership from `/proc`, nothing decides "running" by sleeping and looking,
//! and a launcher that was killed leaves a container marked
//! [`table::State::Dead`] with the time it was NOTICED rather than an exit code
//! nobody measured.
//!
//! ⛔ **What podvm does not have, said once here rather than implied
//! everywhere:** a container is a chroot, not a namespace. There is no PID
//! namespace, so a payload's grandchildren that reparent are outside podvm's
//! reach; `stop` signals the payload and says so.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod launcher;
pub mod table;

use podvm_image::Store;
use table::{Container, State};

/// Everything this crate can fail with, as one sentence for the caller.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ⛔ **docker's codes, from the one file that holds them.**
// [`TODO/cli.md`](../../../TODO/cli.md) T-0802. This crate declared its own 125
// as well, which made FOUR copies of one number in one binary; the deep review
// that follows T-0802's consolidation is what found this one, after the commit
// message had already claimed there was one.
pub use podvm_probe::exit::EXIT_RUNTIME_ERROR;

/// A fresh container id: 64 hex characters, as docker's are.
///
/// ⚠ Not a hash of anything. docker's is random, two containers of one image
/// are different containers, and a content-derived id would collide for exactly
/// the case a lifecycle test drives twenty times.
pub fn new_id() -> String {
    let mut bytes = [0u8; 32];
    // ⛔ `read_exact` OF 32 BYTES, never `std::fs::read`. Measured on
    // 2026-09-09, the expensive way: `std::fs::read("/dev/urandom")` reads to
    // EOF and `/dev/urandom` has none, so `podvm create` allocated 13 GB and
    // was OOM-killed with exit 137. A character device is a stream and a whole-
    // file read is a request for all of it.
    match std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))
    {
        Ok(()) => {}
        Err(_) => {
            // ⚠ A named fallback rather than a silent one: the id still has to
            // be unique, so it is built from the pid and the clock, and it is
            // written here so a reader knows which one they are looking at.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let pid = std::process::id() as u128;
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((now >> (i % 16)) ^ (pid << 3) ^ (i as u128)) as u8;
            }
        }
    };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A container name docker would generate, for a caller that gave none.
pub fn generated_name(id: &str) -> String {
    format!("podvm_{}", &id[..12.min(id.len())])
}

/// Write a `created` record. ⛔ Nothing is started here: `create` and `start`
/// are two verbs because docker's are, and a `create` that started something
/// would make `start` a no-op that reports success.
///
/// `userland_exec` is the detached userland drive TODO/enter.md T-1411
/// records: the exact loader argv, empty everywhere else. The launcher
/// execs it with no chroot where it is non-empty and enters through
/// `spawn_selected` where it is empty.
#[allow(clippy::too_many_arguments)]
pub fn create(
    store: &Store,
    name: Option<&str>,
    image: &str,
    manifest_digest: &str,
    rootfs: &str,
    argv: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    rung: &str,
    completion: Vec<String>,
    completion_degraded: usize,
    userland_exec: Vec<String>,
) -> Result<Container> {
    let id = new_id();
    let name = name
        .map(str::to_string)
        .unwrap_or_else(|| generated_name(&id));
    let c = Container {
        id: id.clone(),
        name: name.clone(),
        image: image.to_string(),
        manifest_digest: manifest_digest.to_string(),
        rootfs: rootfs.to_string(),
        argv,
        env,
        working_dir,
        created_at: podvm_image::clock::now(),
        started_at: None,
        finished_at: None,
        state: State::Created,
        pid: None,
        launcher_pid: None,
        exit_code: None,
        noticed: None,
        rung: rung.to_string(),
        userland_exec,
        completion,
        completion_degraded,
    };
    table::update(store, |t| {
        // ⛔ A name identifies one container. docker refuses a duplicate and so
        // does podvm, rather than making the second one unreachable by name.
        if t.containers.iter().any(|x| x.name == name) {
            return Err(Error(format!(
                "the name {name:?} is already used by a container. Remove it, or \
                 give this one another name"
            )));
        }
        t.containers.push(c.clone());
        Ok(())
    })?;
    // T-0810: the container's directory has to exist before anything is
    // stored beside the record (the memo rename at create, the log and the
    // lock at start). Nothing else makes it, and the rename fails with
    // ENOENT where it does not exist.
    let dir = table::dir(store, &id);
    std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    Ok(c)
}

/// Start a created container, returning once its payload is running.
pub fn start(store: &Store, want: &str, record: &podvm_image::Record) -> Result<Container> {
    let table = table::reconcile(store)?;
    let c = table::find(&table, want)?;
    if c.state == State::Running {
        return Err(Error(format!("{} is already running", c.name)));
    }
    let (launcher, pid) = launcher::start(store, &c, record)?;
    let mut out = c;
    out.state = State::Running;
    out.launcher_pid = Some(launcher);
    out.pid = Some(pid);
    Ok(out)
}

/// Send a signal to a running container's payload.
pub fn kill(store: &Store, want: &str, sig: i32) -> Result<Container> {
    let table = table::reconcile(store)?;
    let c = table::find(&table, want)?;
    if c.state != State::Running {
        return Err(Error(format!(
            "{} is {} and not running, so there is nothing to signal",
            c.name,
            c.state.word()
        )));
    }
    launcher::signal(store, &c.id, sig)?;
    Ok(c)
}

/// SIGTERM, then wait, then SIGKILL. ⛔ Bounded at every step, and each outcome
/// is distinct: `TODO/RULES.md` section 8.
pub fn stop(store: &Store, want: &str, timeout_ms: u64) -> Result<(Container, bool)> {
    let table = table::reconcile(store)?;
    let c = table::find(&table, want)?;
    if c.state != State::Running {
        return Ok((c, false));
    }
    launcher::signal(store, &c.id, 15)?;
    // ⛔ **A LAUNCHER THAT IS ALREADY GONE IS A CONTAINER THAT ALREADY STOPPED**,
    // not a failure. Found by the door sweep of 2026-09-09 and it is the race
    // `experiments/230-lifecycle-loop.sh` kept failing on: `stop` connects
    // TWICE, once to signal and once to wait, and a payload that dies quickly
    // lets the launcher remove its control socket in between. The second
    // connect then answers ENOENT, and reporting that as "the container is not
    // running" turned the fastest possible success into an error. ⚠ Which is
    // why `wait_or_gone` distinguishes "the launcher answered" from "there is
    // no launcher to answer" from "the bound was reached", and only the last
    // means podvm does not know.
    // ⛔ T-1691 (issue 215, call C12): a zero grace is `Ended::Bound` at once,
    // so SIGKILL follows, as `docker stop -t 0` does.
    match wait_or_gone(store, &c.id, timeout_ms)? {
        Ended::Code(_) | Ended::LauncherGone => return Ok((c, false)),
        Ended::Bound => {}
    }
    // ⚠ The bound was reached, which is a fact rather than a failure, and the
    // caller is told that SIGKILL is what ended it.
    launcher::signal(store, &c.id, 9).ok();
    let _ = wait_or_gone(store, &c.id, timeout_ms.max(KILL_WAIT_MS));
    Ok((c, true))
}

/// Why a wait on a launcher ended.
///
/// ⛔ Three states, because collapsing the first two is what made a container
/// that stopped instantly read as one that was never running.
enum Ended {
    /// The launcher answered with the payload's exit code.
    Code(i32),
    /// There is no launcher: it has already finished and torn its socket down.
    LauncherGone,
    /// The bound was reached and podvm does not know.
    Bound,
}

/// How long a wait after SIGKILL may take, when the caller's bound is shorter.
const KILL_WAIT_MS: u64 = 5_000;

fn wait_or_gone(store: &Store, id: &str, timeout_ms: u64) -> Result<Ended> {
    ended(launcher::wait(store, id, timeout_ms))
}

/// The answer of `launcher::wait` as an [`Ended`].
///
/// ⛔ T-1691 (issue 215): only "no launcher answers" is the launcher having
/// gone. Any other failure of the wait is an error for the caller: read as
/// gone, it made `stop` report a running container as stopped.
fn ended(r: std::result::Result<Option<i32>, launcher::WaitError>) -> Result<Ended> {
    match r {
        Ok(Some(c)) => Ok(Ended::Code(c)),
        Ok(None) => Ok(Ended::Bound),
        Err(launcher::WaitError::Gone(_)) => Ok(Ended::LauncherGone),
        Err(launcher::WaitError::Failed(e)) => Err(e),
    }
}

/// Wait for a container to end. `None` is the bound, never a guess.
pub fn wait(store: &Store, want: &str, timeout_ms: u64) -> Result<(Container, Option<i32>)> {
    let table = table::reconcile(store)?;
    let c = table::find(&table, want)?;
    match c.state {
        State::Running => match wait_or_gone(store, &c.id, timeout_ms)? {
            Ended::Code(n) => Ok((c, Some(n))),
            // ⚠ The launcher finished between the reconcile above and this
            // connect. Its last act is to write the exit code into the table,
            // so the answer is there: re-read rather than report the race.
            Ended::LauncherGone => {
                let again = table::reconcile(store)?;
                let c = table::find(&again, want)?;
                let code = c.exit_code;
                Ok((c, code))
            }
            Ended::Bound => Ok((c, None)),
        },
        // ⛔ `Dead` has no exit code and never invents one.
        State::Dead => Ok((c, None)),
        _ => {
            let code = c.exit_code;
            Ok((c, code))
        }
    }
}

/// Remove a container's record and its own directory.
pub fn remove(store: &Store, want: &str, force: bool) -> Result<Container> {
    let table = table::reconcile(store)?;
    let c = table::find(&table, want)?;
    if c.state == State::Running {
        if !force {
            return Err(Error(format!(
                "{} is running. Stop it first, or use -f",
                c.name
            )));
        }
        launcher::signal(store, &c.id, 9).ok();
        let _ = launcher::wait(store, &c.id, KILL_WAIT_MS);
    }
    table::update(store, |t| {
        t.containers.retain(|x| x.id != c.id);
        Ok(())
    })?;
    // ⛔ Gated through the same containment check every other unlink in this
    // tree takes, so a crafted id cannot reach outside the store.
    let dir = table::dir(store, &c.id);
    if let Ok(resolved) = podvm_image::contain::within(store.root(), &dir) {
        let _ = std::fs::remove_dir_all(resolved);
    }
    Ok(c)
}

/// Every container, reconciled.
pub fn list(store: &Store, all: bool) -> Result<Vec<Container>> {
    let table = table::reconcile(store)?;
    Ok(table
        .containers
        .into_iter()
        .filter(|c| all || c.state == State::Running)
        .collect())
}

pub fn get(store: &Store, want: &str) -> Result<Container> {
    let table = table::reconcile(store)?;
    table::find(&table, want)
}

/// Every container record referencing one manifest digest, running or not.
///
/// TODO/image.md T-1322. One query, three callers (`run --rm`, `rmi`,
/// `prune`): docker's removal rule keys on any container, and `create`
/// takes no hold, so only the records see a created container's
/// reference. Reconciled first, so a dead launcher's record reads `Dead`
/// rather than haunting the answer as `Running`.
pub fn referencing(store: &Store, manifest_digest: &str) -> Result<Vec<Container>> {
    let table = table::reconcile(store)?;
    Ok(table
        .containers
        .into_iter()
        .filter(|c| c.manifest_digest == manifest_digest)
        .collect())
}

/// The container's captured output.
pub fn logs(store: &Store, want: &str) -> Result<Vec<u8>> {
    let c = get(store, want)?;
    let path = table::log_path(store, &c.id);
    match std::fs::read(&path) {
        Ok(v) => Ok(v),
        // ⚠ A container that never wrote anything has no log file, and that is
        // an empty log rather than an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error(format!("{}: {e}", path.display()))),
    }
}

/// Follow a container's log: print what is there, then poll-append until
/// the container reaches a terminal state, with one bounded final drain
/// so a write racing the state flip is still shown.
///
/// TODO/supervise.md T-1318. Bounded poll (100 ms), never inotify: no new
/// crate, same behaviour everywhere. A shrink resets the offset rather
/// than erroring; rotation itself is out of scope. A container that never
/// exits follows forever, which is docker's answer too.
pub fn follow(store: &Store, want: &str, out: &mut dyn std::io::Write) -> Result<()> {
    follow_from(store, want, out, 0)
}

/// Follow a container's log from a byte offset: print what is past it,
/// then poll-append exactly as [`follow`] does. TODO/cli.md T-1337:
/// `logs -f --tail N` prints the last N lines, then follows what
/// arrives, instead of replaying the whole file on every reconnect.
/// The offset chains: bytes past it print once here and the poll loop
/// continues past them, so no line is lost or repeated between the two.
pub fn follow_from(
    store: &Store,
    want: &str,
    out: &mut dyn std::io::Write,
    start: u64,
) -> Result<()> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(100);
    const FINAL_DRAIN: std::time::Duration = std::time::Duration::from_millis(200);
    let c = get(store, want)?;
    let path = table::log_path(store, &c.id);
    let mut offset = append_from(&path, start, out)?;
    loop {
        std::thread::sleep(POLL);
        let (next, done) = follow_once(store, want, &path, offset, out)?;
        offset = next;
        if done {
            std::thread::sleep(FINAL_DRAIN);
            append_from(&path, offset, out)?;
            return Ok(());
        }
    }
}

/// The byte offset where the last `n` lines of `bytes` begin.
///
/// TODO/cli.md T-1337. Lines end at `\n`; one trailing newline
/// terminates the last line rather than starting an empty one, so `a\n`
/// and `a` are each one line. Zero lines is the end of the buffer, and
/// more lines than the buffer holds is its start: `--tail` never fails
/// for counting alone.
pub fn tail_offset(bytes: &[u8], n: u64) -> u64 {
    if n == 0 {
        return bytes.len() as u64;
    }
    let body = if bytes.last() == Some(&b'\n') {
        &bytes[..bytes.len() - 1]
    } else {
        bytes
    };
    let mut seen = 0u64;
    let mut i = body.len();
    while i > 0 {
        i -= 1;
        if body[i] == b'\n' {
            seen += 1;
            if seen == n {
                return (i + 1) as u64;
            }
        }
    }
    0
}

/// Follow a container's log from its last `n` lines: print the tail,
/// then follow what arrives. The tail is measured against one read and
/// the poll loop chains past that read's length, so a line landing
/// between the two is followed, never lost and never repeated.
pub fn follow_tail(store: &Store, want: &str, out: &mut dyn std::io::Write, n: u64) -> Result<()> {
    let bytes = logs(store, want)?;
    let start = tail_offset(&bytes, n);
    let end = bytes.len() as u64;
    out.write_all(&bytes[start as usize..])
        .map_err(|e| Error(format!("writing the log: {e}")))?;
    let _ = out.flush();
    follow_from(store, want, out, end)
}
/// One poll step: append what arrived, and whether the container has ended.
/// The final drain stays in [`follow_from`]: a write racing the state flip lands
/// between this step's read and the drain.
///
/// A step rather than a loop so the test below drives arrivals
/// deterministically: `podvm-supervise` spawns no thread anywhere, not
/// even in tests (`launcher::nothing_on_the_spawn_path_can_spawn_a_thread`
/// reads every source line), so no test may race `follow` with a writer
/// thread.
fn follow_once(
    store: &Store,
    want: &str,
    path: &std::path::Path,
    offset: u64,
    out: &mut dyn std::io::Write,
) -> Result<(u64, bool)> {
    let offset = append_from(path, offset, out)?;
    Ok((offset, terminal(get(store, want)?.state)))
}

/// A state `follow` stops at: the run ended, watched or not.
fn terminal(state: table::State) -> bool {
    matches!(state, table::State::Exited | table::State::Dead)
}

/// Write the bytes past `offset` and return the new offset. A missing file
/// is an empty log (see [`logs`]); a shorter file resets the offset.
fn append_from(path: &std::path::Path, offset: u64, out: &mut dyn std::io::Write) -> Result<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(offset),
        Err(e) => return Err(Error(format!("{}: {e}", path.display()))),
    };
    let len = f
        .metadata()
        .map(|m| m.len())
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let mut offset = offset;
    if len < offset {
        offset = len;
    }
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    out.write_all(&buf)
        .map_err(|e| Error(format!("writing the log: {e}")))?;
    let _ = out.flush();
    Ok(offset + buf.len() as u64)
}

#[cfg(test)]
mod tests;
