//! The new binary, run once before it takes the old one's place: a file
//! that does not start on this host (a libc, a CPU feature, a defect) would
//! end an installed node at its next start, and a server that only the
//! node reaches would be locked out. The file runs with `--version` alone,
//! its stdin empty, within a limit; its exit code is read directly, and its
//! first line must be `PROGRAM VERSION`, of the release asked for. Its
//! signature and its sum were checked first, so running it trusts nothing
//! new.
//!
//! The base is podssh's `update/trial.rs`; podnet's `trial` has the same
//! steps.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pod_core::text::one_line;

use crate::version::Version;

/// The bound on a run of `--version`.
pub const LIMIT: Duration = Duration::from_secs(10);

/// What a trial run expects: the first word of the line (the program's
/// name, as `podssh`), the version (any version, with none), and the limit.
#[derive(Debug, Clone)]
pub struct Expect<'a> {
    pub program: &'a str,
    pub version: Option<&'a Version>,
    pub limit: Duration,
}

/// Run `file --version` within `expect.limit`: its line, `PROGRAM VERSION`,
/// when it ended with 0 and named the version expected; else why not.
pub fn check(file: &Path, expect: &Expect<'_>) -> Result<String, String> {
    let mut child = spawn(file)?;
    // Read as it writes, so that a long answer cannot hold it.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(64 << 10).read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + expect.limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it did not end within {} ms", expect.limit.as_millis()));
            }
            Err(e) => return Err(format!("its wait: {e}")),
        }
    };
    let out = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    if !status.success() {
        let said = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
        let said = said.lines().map(str::trim).find(|l| !l.is_empty()).map(one_line);
        return Err(match said {
            Some(said) => format!("it ended with {status}: {said}"),
            None => format!("it ended with {status}"),
        });
    }
    let line = out.lines().next().unwrap_or("").trim().to_string();
    let program = expect.program;
    // `PROGRAM VERSION`, and more words after it in a release build.
    let named = line
        .strip_prefix(program)
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(Version::parse);
    match (named, expect.version) {
        (None, _) => Err(format!("it said {:?}, not {program} VERSION", one_line(&line))),
        (Some(named), Some(want)) if &named != want => Err(format!("it is {program} {named}, not {want}")),
        (Some(_), _) => Ok(line),
    }
}

/// `file --version`, started again for a while when another thread of this
/// process holds the new file open between its fork and its exec (ETXTBSY).
fn spawn(file: &Path) -> Result<Child, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut cmd = Command::new(file);
        cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e) if busy(&e) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("it does not start here: {e}")),
        }
    }
}

#[cfg(unix)]
fn busy(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn busy(_: &std::io::Error) -> bool {
    false
}
