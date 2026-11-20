//! A saved state of any machine line, with no [`crate::Plan`]: the save
//! and the load through QMP, a record beside the stream, and the
//! fingerprint of the line. The UEFI flavour ([`super::save`],
//! [`super::restore`]) and a guest of another line (a direct kernel boot
//! with 9p devices) share these steps.
//!
//! * [`freeze`]: the check of the devices, QMP `stop`, the stream into one
//!   file, `cont`; the caller's [`Stopped`] hooks run while the guest is
//!   stopped (a check of the guest before the stream, a copy of its disks
//!   after it).
//! * [`save`]: [`freeze`] into `STREAM.part-UNIQUE`, a sync, the [`Record`]
//!   into `RECORD.part-UNIQUE`, then the renames: a reader finds a whole
//!   state or none.
//! * [`check`]: the record of a state whose fingerprint is the text of this
//!   run and whose stream has the length of its record; else a failure with
//!   the cause [`super::Cold`].
//! * [`incoming`]: the line of the caller, then `-incoming` and the stream.
//! * [`running`]: the wait for the end of the load, then `cont`.
//! * [`Inputs`]: the text of a fingerprint of a line.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure};

use crate::{Result, failure, io_at, qmp};

mod inputs;
mod record;

pub use inputs::{Inputs, cpu};
pub use record::{RECORD, Record, check, first_difference};

/// The time between two polls of the emulator.
pub(crate) const EVERY: Duration = Duration::from_millis(50);
/// The wait for the QMP socket of a guest that runs.
pub(crate) const MONITOR_WAIT: Duration = Duration::from_secs(5);
/// The rate of the save, in bytes each second. The guest is stopped, so
/// the default limit of the emulator only makes the save slower.
const BANDWIDTH: u64 = 1 << 40;
const QUERY_MIGRATE: &str = "{\"execute\":\"query-migrate\"}";

/// How the stream goes to its file and comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// The emulator writes and reads the file (`file:`, QEMU 8.2 and later).
    File,
    /// The emulator writes to and reads from a pipe of `cat` (`exec:`), the
    /// path of a QEMU before 8.2.
    Pipe,
}

impl Transport {
    /// `file` or `pipe`, as a [`Record`] holds it.
    pub fn word(self) -> &'static str {
        match self {
            Transport::File => "file",
            Transport::Pipe => "pipe",
        }
    }

    /// The transport of a word of [`Transport::word`].
    pub fn of(word: &str) -> Option<Transport> {
        match word {
            "file" => Some(Transport::File),
            "pipe" => Some(Transport::Pipe),
            _ => None,
        }
    }

    /// The URI of a save into `path`. A path that is not UTF-8, or a
    /// `file:` path with a comma (the emulator reads `,offset=` after the
    /// path), is [`Class::Usage`].
    pub fn save_uri(self, path: &Path) -> Result<String> {
        match self {
            Transport::File => Ok(format!("file:{}", file_path(path)?)),
            Transport::Pipe => Ok(format!("exec:cat > {}", sh_quote(utf8(path)?))),
        }
    }

    /// The URI of a load from `path`, with the refusals of
    /// [`Transport::save_uri`].
    pub fn load_uri(self, path: &Path) -> Result<String> {
        match self {
            Transport::File => Ok(format!("file:{}", file_path(path)?)),
            Transport::Pipe => Ok(format!("exec:cat {}", sh_quote(utf8(path)?))),
        }
    }
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| failure(Class::Usage, format!("{}: a saved state needs a UTF-8 path", path.display())))
}

fn file_path(path: &Path) -> Result<&str> {
    match path.to_str() {
        Some(p) if !p.contains(',') => Ok(p),
        _ => Err(failure(Class::Usage, format!("{}: a saved state needs a UTF-8 path with no comma", path.display()))),
    }
}

/// `text` as one word of `sh`: in single quotes, each quote as `'\''`.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The caller's steps while the guest is stopped for a save. An error of a
/// hook ends the save, and the guest goes on.
pub trait Stopped {
    /// After `stop`, before the stream: a check of the guest, for example
    /// of a mailbox. The default does nothing.
    fn before(&mut self, monitor: &mut qmp::Monitor) -> Result<()> {
        let _ = monitor;
        Ok(())
    }

    /// After the stream, before `cont`: the files of the guest are the ones
    /// of the stream, for example a copy of its disks. The default does
    /// nothing.
    fn after(&mut self, monitor: &mut qmp::Monitor) -> Result<()> {
        let _ = monitor;
        Ok(())
    }
}

/// No step while the guest is stopped.
impl Stopped for () {}

/// One stream of a guest that runs, for [`freeze`].
#[derive(Debug, Clone, Copy)]
pub struct Freeze<'a> {
    /// The QMP socket of the emulator.
    pub monitor: &'a Path,
    /// The file of the stream.
    pub stream: &'a Path,
    /// The first transport to try.
    pub transport: Transport,
    /// A `file:` save that the emulator refuses or fails is tried again
    /// through the pipe ([`Transport::Pipe`]).
    pub fallback: bool,
    /// The bound of each migration; a migration past it is cancelled.
    pub wait: Duration,
}

/// The end of [`freeze`].
#[derive(Debug)]
pub struct Frozen {
    /// The transport that wrote the stream, or why there is no stream.
    pub stream: Result<Transport>,
    /// `Ok` where the guest runs after the save: `cont` worked, or the
    /// save did not stop the guest.
    pub goes_on: Result<()>,
}

/// Stop the guest behind `f.monitor`, write its stream to `f.stream`, run
/// `hooks`, and let the guest go on.
///
/// An emulator that cannot migrate a device (`blocked-reasons` of
/// `query-migrate`) is [`Class::Unavailable`] with its reasons, before the
/// guest stops. A failed or cancelled migration is [`Class::Protocol`]; a
/// migration that does not end within `f.wait` is cancelled
/// ([`Class::Timeout`]). The guest goes on after each failure that came
/// after the stop. The caller removes a stream of a failed save.
pub fn freeze(f: &Freeze, hooks: &mut dyn Stopped) -> Frozen {
    let before_stop = |e: Failure| Frozen { stream: Err(e), goes_on: Ok(()) };
    if let Err(e) = f.transport.save_uri(f.stream) {
        return before_stop(e);
    }
    let mut m = match qmp::Monitor::connect(f.monitor, MONITOR_WAIT) {
        Ok(m) => m,
        Err(e) => return before_stop(e),
    };
    match m.ask(QUERY_MIGRATE) {
        Ok(line) => {
            if let Some(reasons) = blocked(&line) {
                return before_stop(failure(
                    Class::Unavailable,
                    format!("the emulator cannot save this guest: {reasons}"),
                ));
            }
        }
        Err(e) => return before_stop(e),
    }
    if let Err(e) = m.call("stop") {
        let goes_on = m.call("cont");
        return Frozen { stream: Err(e), goes_on };
    }
    let stream = stopped(&mut m, f, hooks);
    // The guest goes on in each case: a failed save leaves it as it was.
    let goes_on = m.call("cont");
    Frozen { stream, goes_on }
}

/// The steps of [`freeze`] between `stop` and `cont`.
fn stopped(m: &mut qmp::Monitor, f: &Freeze, hooks: &mut dyn Stopped) -> Result<Transport> {
    hooks.before(m)?;
    // A refusal keeps the default rate: the save is then slower.
    let _ =
        m.ask(&format!("{{\"execute\":\"migrate-set-parameters\",\"arguments\":{{\"max-bandwidth\":{BANDWIDTH}}}}}"));
    let mut transport = f.transport;
    let mut made = migrate(m, transport, f.stream, f.wait);
    // A QEMU before 8.2 has no `file:`: it refuses the URI. A timeout or a
    // lost monitor is not a matter of the transport.
    if let Err(file) = &made
        && transport == Transport::File
        && f.fallback
        && file.class == Class::Protocol
    {
        let _ = std::fs::remove_file(f.stream);
        transport = Transport::Pipe;
        made = migrate(m, transport, f.stream, f.wait)
            .map_err(|pipe| failure(pipe.class, format!("{file}; through a pipe: {pipe}")));
    }
    made?;
    hooks.after(m)?;
    Ok(transport)
}

/// The text of `"blocked-reasons"` of a `query-migrate` response.
fn blocked(line: &str) -> Option<String> {
    let at = line.find("\"blocked-reasons\"")?;
    let rest = &line[at..];
    let list = &rest[rest.find('[')? + 1..rest.find(']')?];
    let list = list.trim();
    (!list.is_empty()).then(|| list.to_string())
}

/// One `migrate` of the stopped guest into `stream`, to its end.
fn migrate(m: &mut qmp::Monitor, transport: Transport, stream: &Path, wait: Duration) -> Result<()> {
    let uri = transport.save_uri(stream)?;
    m.ask(&format!("{{\"execute\":\"migrate\",\"arguments\":{{\"uri\":{}}}}}", qmp::json_text(&uri)))?;
    let start = Instant::now();
    loop {
        let line = m.ask(QUERY_MIGRATE)?;
        match qmp::json_member(&line, "status").as_deref() {
            Some("completed") => return Ok(()),
            Some(end @ ("failed" | "cancelled")) => {
                let why = qmp::json_member(&line, "error-desc").unwrap_or_default();
                return Err(failure(Class::Protocol, format!("the save {end}: {why}")));
            }
            _ => {}
        }
        if start.elapsed() >= wait {
            let _ = m.call("migrate_cancel");
            return Err(failure(Class::Timeout, format!("the save did not end within {}s", wait.as_secs())));
        }
        std::thread::sleep(EVERY);
    }
}

/// One saved state of a guest that runs, for [`save`].
#[derive(Debug, Clone, Copy)]
pub struct Save<'a> {
    /// The QMP socket of the emulator.
    pub monitor: &'a Path,
    /// The file of the stream.
    pub stream: &'a Path,
    /// The file of the [`Record`].
    pub record: &'a Path,
    /// The text of the fingerprint of this run ([`Inputs::text`]).
    pub fingerprint: &'a str,
    /// Values that the load needs, as `(key, value)`: a key is a word of
    /// letters, digits, `-` and `_`, a value has no line break.
    pub notes: &'a [(String, String)],
    /// The first transport to try.
    pub transport: Transport,
    /// As [`Freeze::fallback`].
    pub fallback: bool,
    /// The name of the partial files of this save (`STREAM.part-UNIQUE`,
    /// `RECORD.part-UNIQUE`): a nonce of the run, with no `/`.
    pub unique: &'a str,
    /// As [`Freeze::wait`].
    pub wait: Duration,
}

/// A save that ended well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    /// The length of the stream.
    pub bytes: u64,
    /// The time from the call to the rename.
    pub took: Duration,
    /// The transport that wrote the stream.
    pub transport: Transport,
}

/// The end of [`save`].
#[derive(Debug)]
pub struct SaveEnd {
    /// The state, or why there is none.
    pub saved: Result<Saved>,
    /// As [`Frozen::goes_on`]: where it is an error, the caller boots
    /// another guest.
    pub goes_on: Result<()>,
}

/// Save the guest behind `s.monitor` into `s.stream` and its record into
/// `s.record`, and let the guest go on ([`freeze`] with `hooks`).
///
/// The stream goes to `STREAM.part-UNIQUE` and is synced. The record
/// ([`Record`]: the transport, the length, the notes and the
/// fingerprint) goes to `RECORD.part-UNIQUE` and is synced. The record of
/// the state before goes away first, then both files get their names, and
/// the directory is synced: a reader pairs no record with the stream of
/// another save, and finds a whole state or none. A save that fails
/// removes its partial files and keeps the state before. An empty stream is
/// [`Class::Protocol`]; a bad note or a bad `unique` is [`Class::Usage`].
pub fn save(s: &Save, hooks: &mut dyn Stopped) -> SaveEnd {
    let started = Instant::now();
    let usage = |e: Failure| SaveEnd { saved: Err(e), goes_on: Ok(()) };
    if s.unique.is_empty() || s.unique.contains('/') || s.unique.contains('\0') {
        return usage(failure(Class::Usage, format!("{:?}: the name of a partial file of a save", s.unique)));
    }
    if let Err(e) = record::check_notes(s.notes) {
        return usage(e);
    }
    let part = partial(s.stream, s.unique);
    let record_part = partial(s.record, s.unique);
    let f = Freeze { monitor: s.monitor, stream: &part, transport: s.transport, fallback: s.fallback, wait: s.wait };
    let frozen = freeze(&f, hooks);
    let saved = frozen.stream.and_then(|transport| {
        let bytes = sync(&part)?;
        if bytes == 0 {
            return Err(failure(Class::Protocol, format!("{}: the emulator wrote no byte", part.display())));
        }
        let r = Record { transport, bytes, notes: s.notes.to_vec(), fingerprint: s.fingerprint.to_string() };
        std::fs::write(&record_part, r.text()).map_err(|e| io_at(&record_part, e))?;
        sync(&record_part)?;
        let _ = std::fs::remove_file(s.record);
        std::fs::rename(&part, s.stream).map_err(|e| io_at(s.stream, e))?;
        std::fs::rename(&record_part, s.record).map_err(|e| io_at(s.record, e))?;
        if let Some(dir) = s.record.parent().and_then(|d| std::fs::File::open(d).ok()) {
            let _ = dir.sync_all();
        }
        Ok(Saved { bytes, took: started.elapsed(), transport })
    });
    if saved.is_err() {
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::remove_file(&record_part);
    }
    SaveEnd { saved, goes_on: frozen.goes_on }
}

/// `path` with `.part-UNIQUE` after its name.
fn partial(path: &Path, unique: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".part-{unique}"));
    PathBuf::from(name)
}

/// Sync `path` to the disk, and give its length.
fn sync(path: &Path) -> Result<u64> {
    let file = std::fs::File::open(path).map_err(|e| io_at(path, e))?;
    file.sync_all().map_err(|e| io_at(path, e))?;
    Ok(file.metadata().map_err(|e| io_at(path, e))?.len())
}

/// The machine line of a restore: `line` (the caller's line, with no
/// program), then `-incoming` and the URI of `stream` by `transport`.
pub fn incoming(line: &[String], transport: Transport, stream: &Path) -> Result<Vec<String>> {
    let mut a = line.to_vec();
    a.push("-incoming".to_string());
    a.push(transport.load_uri(stream)?);
    Ok(a)
}

/// Wait for the end of the load of the emulator behind `monitor` within
/// `wait`, then let the guest go on: `query-status` is `inmigrate` during
/// the load, then `paused` (the guest was stopped at the save),
/// `prelaunch` or `postmigrate` (each gets `cont`), or `running`.
///
/// A connection that fails is made again at the next poll: the socket can
/// come after the spawn, and an emulator can answer only after the load.
/// `child` is the emulator where the caller holds it: an exit during the
/// load is [`Class::Unavailable`]. Another status is [`Class::Protocol`];
/// no end within `wait` is [`Class::Timeout`] with the last error of the
/// monitor.
pub fn running(monitor: &Path, mut child: Option<&mut Child>, wait: Duration) -> Result<()> {
    let start = Instant::now();
    let mut m: Option<qmp::Monitor> = None;
    let mut last = String::new();
    loop {
        if let Some(c) = child.as_deref_mut()
            && let Some(status) = c.try_wait().map_err(|e| io_at(monitor, e))?
        {
            return Err(failure(Class::Unavailable, format!("the emulator exited ({status}) during the load")));
        }
        if m.is_none() {
            match qmp::Monitor::connect(monitor, Duration::ZERO) {
                Ok(c) => m = Some(c),
                Err(e) => last = e.to_string(),
            }
        }
        if let Some(c) = m.as_mut() {
            match c.ask("{\"execute\":\"query-status\"}") {
                Ok(line) => match qmp::json_member(&line, "status").as_deref() {
                    Some("inmigrate") => {}
                    Some("running") => return Ok(()),
                    Some("paused" | "prelaunch" | "postmigrate") => return c.call("cont"),
                    other => {
                        return Err(failure(Class::Protocol, format!("the emulator is {other:?} after the load")));
                    }
                },
                Err(e) => {
                    last = e.to_string();
                    m = None;
                }
            }
        }
        if start.elapsed() >= wait {
            let why = if last.is_empty() { String::new() } else { format!(" (the last error: {last})") };
            return Err(failure(Class::Timeout, format!("the load did not end within {}s{why}", wait.as_secs())));
        }
        std::thread::sleep(EVERY);
    }
}

#[cfg(test)]
mod tests;
