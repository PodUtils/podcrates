//! One run of a program within a time limit, with its output read while it
//! runs: a program that writes more than a pipe holds cannot block the
//! wait, and a program that does not end is killed at the limit. The
//! caller gives the program, its arguments and its stdin; no shell reads
//! them.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The most bytes of stdout and of stderr that a run keeps: a program that
/// writes more cannot fill the memory of the caller.
pub const KEPT: u64 = 1 << 20;

/// What a run gave: its exit code (none when a signal ended it), its stdout
/// and its stderr, each cut at [`KEPT`] bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// True when the program ended with 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// The first line of stderr that is not blank, else of stdout: the words
    /// of a failure for one line of the caller.
    pub fn said(&self) -> String {
        let first = |text: &str| text.lines().map(str::trim).find(|l| !l.is_empty()).map(crate::text::one_line);
        first(&self.stderr).or_else(|| first(&self.stdout)).unwrap_or_default()
    }
}

/// Why a run gave no [`Ran`].
#[derive(Debug)]
pub enum RunFault {
    /// The program did not start (not found, no permission).
    Start(std::io::Error),
    /// The program ran past the limit, and was killed.
    Limit(Duration),
    /// The wait for the program failed.
    Wait(std::io::Error),
}

impl std::fmt::Display for RunFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFault::Start(e) => write!(f, "it did not start: {e}"),
            RunFault::Limit(limit) => write!(f, "it did not end within {} ms, and was killed", limit.as_millis()),
            RunFault::Wait(e) => write!(f, "its wait failed: {e}"),
        }
    }
}

impl std::error::Error for RunFault {}

/// Run `program` with `args` and `input` on its stdin (none: an empty
/// stdin), for at most `limit`.
pub fn run_bounded(program: &str, args: &[&str], input: Option<&[u8]>, limit: Duration) -> Result<Ran, RunFault> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunFault::Start)?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let writer = match (input, child.stdin.take()) {
        (Some(bytes), Some(mut stdin)) => {
            let bytes = bytes.to_vec();
            // A program that does not read its stdin closes the pipe: the
            // error of the write is no failure of the run.
            Some(std::thread::spawn(move || drop(stdin.write_all(&bytes))))
        }
        _ => None,
    };
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunFault::Limit(limit));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunFault::Wait(e));
            }
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let text = |pipe: JoinHandle<Vec<u8>>| String::from_utf8_lossy(&pipe.join().unwrap_or_default()).into_owned();
    Ok(Ran { code: status.code(), stdout: text(stdout), stderr: text(stderr) })
}

/// Read `pipe` to its end in a thread, and keep [`KEPT`] bytes of it.
fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = (&mut pipe).take(KEPT).read_to_end(&mut kept);
            // The rest is read and dropped, so that the program does not
            // block on a full pipe.
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        kept
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_run_gives_its_code_its_output_and_reads_its_input() {
        let ran = run_bounded("sh", &["-c", "cat; echo err >&2; exit 3"], Some(b"in\n"), Duration::from_secs(10))
            .expect("a run");
        assert_eq!(ran.code, Some(3));
        assert_eq!(ran.stdout, "in\n");
        assert_eq!(ran.said(), "err");
        assert!(!ran.success());
    }

    #[test]
    fn a_run_past_its_limit_is_killed() {
        let started = Instant::now();
        let fault = run_bounded("sh", &["-c", "sleep 30"], None, Duration::from_millis(200)).expect_err("a limit");
        assert!(matches!(fault, RunFault::Limit(_)), "{fault}");
        assert!(started.elapsed() < Duration::from_secs(20), "the kill did not end the wait");
    }

    #[test]
    fn a_program_that_is_not_there_does_not_start() {
        let fault = run_bounded("/nonexistent/program", &[], None, Duration::from_secs(1)).expect_err("no program");
        assert!(matches!(fault, RunFault::Start(_)), "{fault}");
    }

    #[test]
    fn a_long_output_does_not_block_the_run() {
        let ran =
            run_bounded("sh", &["-c", "head -c 3000000 /dev/zero"], None, Duration::from_secs(20)).expect("a run");
        assert_eq!(ran.stdout.len() as u64, KEPT);
        assert!(ran.success());
    }
}
