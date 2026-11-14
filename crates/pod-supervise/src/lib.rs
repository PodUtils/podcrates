//! Detached payloads: one launcher for each payload, a table of records, and
//! the verbs of the lifecycle (create, start, kill, stop, wait, remove, the
//! logs).
//!
//! The running state is the state of the launcher. [`table`] is the record,
//! and [`launcher`] is the only writer of `running` into it, with the
//! [`foreground()`] hold of a payload that the caller waits for itself: the
//! hold takes the place of the launcher (the lock, the control socket, the
//! end). Nothing reads
//! membership from `/proc`, and nothing decides "running" by a sleep and a
//! look. A launcher that was killed leaves its record
//! [`table::State::Dead`], with the time that a later call noticed it, not
//! an exit code that nothing measured.
//!
//! A payload is not in a PID namespace. The launcher is the child
//! subreaper of the payload's tree: a descendant whose parent ends becomes
//! a child of the launcher, also after a `setsid`. At the end of the
//! payload the launcher kills each descendant that stays
//! ([`Settings::kill_tree`]), and writes the count that stays after the
//! kill, the use of the tree and the reason of the end into the record
//! ([`end::End`]). `stop` and `kill` signal the payload only.
//!
//! The crate is Linux only. On another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod end;
pub mod foreground;
pub mod launcher;
pub mod lock;
pub mod logs;
pub mod table;

use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};
use serde::{Serialize, de::DeserializeOwned};
use table::{Container, State};

pub use foreground::{Foreground, foreground};
pub use logs::{follow, follow_from, follow_tail, logs, tail_offset};

pub type Result<T> = std::result::Result<T, Failure>;

/// The values that the caller gives. The caller fills it one time and
/// gives it to each call.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The directory of the state: the table, a directory for each
    /// container, and `staging/` for the writes of the table.
    pub root: PathBuf,
    /// The lock that each write of the table holds. A caller that keeps
    /// other records in `root` gives the lock of those records, so that
    /// the two writers do not interleave.
    pub table_lock: PathBuf,
    /// The key of the version number in the table file.
    pub table_key: String,
    /// The prefix of a generated name: `PREFIX_` and 12 characters of the
    /// id.
    pub name_prefix: String,
    /// The command that lists the containers, for the message of a name
    /// that is not found. `None`: the message names no command.
    pub list_command: Option<String>,
    /// The exit code of a payload that the launcher could not reap. docker
    /// gives 125, its code for a failure of the runtime.
    pub failed_code: i32,
    /// Kill each descendant that stays at the end of the payload, as the
    /// end of PID 1 of a namespace does. `false` only counts them: the
    /// processes then go on with no supervisor.
    pub kill_tree: bool,
    /// The mount of procfs that the launcher reads for the descendants of
    /// the payload.
    pub proc: PathBuf,
}

impl Settings {
    /// The settings for the state in `root`, with the lock `root/lock`.
    pub fn new(root: impl Into<PathBuf>) -> Settings {
        let root = root.into();
        Settings {
            table_lock: root.join("lock"),
            root,
            table_key: "pod_supervise".to_string(),
            name_prefix: "pod".to_string(),
            list_command: None,
            failed_code: 125,
            kill_tree: true,
            proc: PathBuf::from("/proc"),
        }
    }
}

/// An I/O failure at `path`: the text is `PATH: CAUSE`, and the cause
/// stays in the chain.
pub(crate) fn io_at(path: &Path, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("{}: {e}", path.display()), e)
}

/// A private directory (mode 0700) and each missing parent: the state
/// holds the environment of each record, and the log of each payload.
pub(crate) fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

/// A failure of the state, with no I/O cause.
pub(crate) fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, text)
}

/// A new container id: 64 hex characters, as docker's are.
///
/// It is not a hash of a value: two containers of one image are two
/// containers, and an id from the content would be the same for both.
pub fn new_id() -> String {
    let mut bytes = [0u8; 32];
    // `read_exact` of 32 bytes, never `std::fs::read`: `/dev/urandom` has no
    // end, and a read to the end allocates until the process is killed.
    match std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes)) {
        Ok(()) => {}
        Err(_) => {
            // The id must still be unique: it comes from the pid and the
            // clock.
            let now =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let pid = std::process::id() as u128;
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((now >> (i % 16)) ^ (pid << 3) ^ (i as u128)) as u8;
            }
        }
    };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The name for a container that the caller gave no name: as docker
/// generates one, `PREFIX_` and the first 12 characters of the id.
pub fn generated_name(s: &Settings, id: &str) -> String {
    format!("{}_{}", s.name_prefix, &id[..12.min(id.len())])
}

/// Write a `created` record, with the fields `of` of the caller. Nothing
/// starts here: `create` and `start` are two verbs, and a `create` that
/// started a payload would make `start` a step that does nothing.
pub fn create<X>(
    s: &Settings,
    name: Option<&str>,
    argv: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    of: X,
) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let id = new_id();
    let name = name.map(str::to_string).unwrap_or_else(|| generated_name(s, &id));
    let c = Container {
        id: id.clone(),
        name: name.clone(),
        argv,
        env,
        working_dir,
        created_at: table::now(),
        started_at: None,
        finished_at: None,
        state: State::Created,
        pid: None,
        launcher_pid: None,
        exit_code: None,
        noticed: None,
        wall_ms: None,
        end: None,
        of,
    };
    table::update(s, |t: &mut table::Table<X>| {
        // A name identifies one container. docker refuses a second one, and
        // so does this crate: the second would be out of reach by name.
        if t.containers.iter().any(|x| x.name == name) {
            return Err(failure(
                Class::Usage,
                format!("the name {name:?} is already used by a container. Remove it, or give this one another name"),
            ));
        }
        t.containers.push(c.clone());
        Ok(())
    })?;
    // The directory of the container exists before anything is put beside
    // the record: a file of the caller at create, the log and the lock at
    // start.
    let dir = table::dir(s, &id);
    private_dir(&dir).map_err(|e| io_at(&dir, e))?;
    Ok(c)
}

/// Set the wall bound of a container that does not run: at `wall_ms` after
/// the start of the payload, the launcher kills the payload and its tree,
/// and the end reads [`end::Reason::Wall`]. `None` removes the bound.
pub fn set_wall<X>(s: &Settings, want: &str, wall_ms: Option<u64>) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    if c.state == State::Running {
        return Err(failure(Class::Usage, format!("{} is running: set its wall bound before the start", c.name)));
    }
    table::update(s, |t: &mut table::Table<X>| {
        if let Some(x) = t.containers.iter_mut().find(|x| x.id == c.id) {
            x.wall_ms = wall_ms;
        }
        Ok::<_, Failure>(())
    })?;
    get::<X>(s, &c.id)
}

/// Start a created container, and return when its payload runs. `enter`
/// starts the payload in the launcher ([`launcher::start`]).
pub fn start<X, F>(s: &Settings, want: &str, enter: F) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
    F: FnOnce(&Container<X>, launcher::Launch<'_>) -> std::result::Result<launcher::Payload, String>,
{
    let table = table::reconcile::<X>(s)?;
    let c = table::find(s, &table, want)?;
    if c.state == State::Running {
        return Err(failure(Class::Usage, format!("{} is already running", c.name)));
    }
    let (launcher, pid) = launcher::start(s, &c, enter)?;
    let mut out = c;
    out.mark_running(pid, launcher);
    Ok(out)
}

/// Send a signal to the payload of a running container.
pub fn kill<X>(s: &Settings, want: &str, sig: i32) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    if c.state != State::Running {
        return Err(failure(
            Class::Usage,
            format!("{} is {} and not running, so there is nothing to signal", c.name, c.state.word()),
        ));
    }
    launcher::signal(s, &c.id, sig)?;
    Ok(c)
}

/// SIGTERM, then a wait, then SIGKILL. Each step has a bound, and each
/// result is different. The `bool` is true when SIGKILL ended the payload.
pub fn stop<X>(s: &Settings, want: &str, timeout_ms: u64) -> Result<(Container<X>, bool)>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    if c.state != State::Running {
        return Ok((c, false));
    }
    launcher::signal(s, &c.id, 15)?;
    // A launcher that is gone is a container that stopped, not a failure.
    // `stop` connects two times, to signal and to wait, and a payload that
    // ends fast lets the launcher remove its socket in between. So the wait
    // tells "the launcher answered" from "no launcher answers" from "the
    // bound came"; only the last means that the result is not known.
    // A zero grace is `Ended::Bound` at once, so SIGKILL follows, as
    // `docker stop -t 0` does.
    match wait_or_gone(s, &c.id, timeout_ms)? {
        Ended::Code(_) | Ended::LauncherGone => return Ok((c, false)),
        Ended::Bound => {}
    }
    // The bound came: that is a fact, not a failure, and the caller learns
    // that SIGKILL ended the payload.
    launcher::signal(s, &c.id, 9).ok();
    let _ = wait_or_gone(s, &c.id, timeout_ms.max(KILL_WAIT_MS));
    Ok((c, true))
}

/// Why a wait on a launcher ended. Three states: with the first two as one,
/// a container that stopped at once reads as one that never ran.
enum Ended {
    /// The launcher answered with the exit code of the payload.
    Code(i32),
    /// No launcher answers: it ended and removed its socket.
    LauncherGone,
    /// The bound came, and the result is not known.
    Bound,
}

/// How long a wait after SIGKILL can take, when the bound of the caller is
/// shorter.
pub const KILL_WAIT_MS: u64 = 5_000;

/// After a launcher is gone, the reads of its record until the lock is
/// free: at most `GONE_READS` reads, `GONE_READ_MS` apart (1 s).
const GONE_READS: u32 = 50;
const GONE_READ_MS: u64 = 20;

fn wait_or_gone(s: &Settings, id: &str, timeout_ms: u64) -> Result<Ended> {
    ended(launcher::wait(s, id, timeout_ms))
}

/// The answer of [`launcher::wait`] as an [`Ended`]. Only "no launcher
/// answers" is a launcher that is gone. Each other failure of the wait goes
/// to the caller: read as gone, it made `stop` report a running container
/// as stopped.
fn ended(r: std::result::Result<Option<i32>, launcher::WaitError>) -> Result<Ended> {
    match r {
        Ok(Some(c)) => Ok(Ended::Code(c)),
        Ok(None) => Ok(Ended::Bound),
        Err(launcher::WaitError::Gone(_)) => Ok(Ended::LauncherGone),
        Err(launcher::WaitError::Failed(e)) => Err(e),
    }
}

/// Wait for a container to end. `None` is the bound, never a guess.
pub fn wait<X>(s: &Settings, want: &str, timeout_ms: u64) -> Result<(Container<X>, Option<i32>)>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    match c.state {
        State::Running => match wait_or_gone(s, &c.id, timeout_ms)? {
            Ended::Code(n) => Ok((c, Some(n))),
            // The launcher ended between the reconcile and this connect. Its
            // last act writes the exit code into the table, so the answer is
            // there: read it again.
            Ended::LauncherGone => {
                // The kernel closes the socket and the lock of an ended
                // launcher one after the other: a read between the two
                // still sees `Running`, so read again for a short time.
                let mut c = get::<X>(s, want)?;
                for _ in 0..GONE_READS {
                    if c.state != State::Running {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(GONE_READ_MS));
                    c = get::<X>(s, want)?;
                }
                let code = c.exit_code;
                Ok((c, code))
            }
            Ended::Bound => Ok((c, None)),
        },
        // `Dead` has no exit code, and the crate makes none up.
        State::Dead => Ok((c, None)),
        _ => {
            let code = c.exit_code;
            Ok((c, code))
        }
    }
}

/// Remove the record of a container and its own directory. `force` kills a
/// running payload first.
pub fn remove<X>(s: &Settings, want: &str, force: bool) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    if c.state == State::Running {
        if !force {
            return Err(failure(Class::Usage, format!("{} is running. Stop it first, or use -f", c.name)));
        }
        launcher::signal(s, &c.id, 9).ok();
        let _ = launcher::wait(s, &c.id, KILL_WAIT_MS);
    }
    table::update(s, |t: &mut table::Table<X>| {
        t.containers.retain(|x| x.id != c.id);
        Ok(())
    })?;
    if let Some(dir) = table::contained(s, &c.id) {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(c)
}

/// Each container, reconciled; `all` false gives the running ones only.
pub fn list<X>(s: &Settings, all: bool) -> Result<Vec<Container<X>>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    matching(s, |c: &Container<X>| all || c.state == State::Running)
}

/// The container that `want` names (a name, a full id, or a prefix of one
/// id), reconciled.
pub fn get<X>(s: &Settings, want: &str) -> Result<Container<X>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let table = table::reconcile::<X>(s)?;
    table::find(s, &table, want)
}

/// Each record that `keep` accepts, running or not. The table is
/// reconciled first, so that the record of a dead launcher reads `Dead`,
/// not `Running`.
pub fn matching<X>(s: &Settings, keep: impl Fn(&Container<X>) -> bool) -> Result<Vec<Container<X>>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let table = table::reconcile::<X>(s)?;
    Ok(table.containers.into_iter().filter(|c| keep(c)).collect())
}

#[cfg(test)]
mod tests;
