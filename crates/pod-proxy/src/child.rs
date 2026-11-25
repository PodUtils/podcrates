//! A proxy at a command: its stdin and its stdout as one byte stream, and
//! its stderr through a pipe, of which the last bytes stay for the cause of
//! a failure. A command that ends before it answers gives
//! [`DialError::ProxyExited`]; a command that the shell cannot run or does
//! not find is not started again for [`EXEC_HOLD`].

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

use crate::error::DialError;
use crate::proxy::{Carrier, Proxy, command_word};
use crate::stream::Stream;

/// The most bytes of the stderr of a command that a [`ChildIo`] keeps: the
/// last ones, which hold the cause of a failure.
pub const STDERR_KEPT: usize = 512;

/// How long a command that exited 126 or 127 (9009 on Windows) is not
/// started again: each dial through it in this time gets the same failure
/// at once, and a loop of dials does not start a process for each one.
pub const EXEC_HOLD: Duration = Duration::from_secs(10);

/// The longest wait for the exit of a command that closed its stdout.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// The stdin and the stdout of a command as one byte stream. The command
/// runs with `sh -c` on Unix and with `cmd.exe /D /S /C` on Windows. Its
/// stderr goes to a pipe, never to the stderr of the process: a task of the
/// runtime reads it and keeps its last [`STDERR_KEPT`] bytes
/// ([`ChildIo::stderr_tail`]). On the runtime of the blocking face, that
/// task runs only in a call: between two calls, a command that writes more
/// than a pipe holds to its stderr waits, as one that writes to its stdout
/// does. The end of the stream closes its stdin; a drop kills it.
pub struct ChildIo {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    stdout: tokio::process::ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    /// The task that reads the stderr; it ends at the end of the stderr.
    drain: Option<tokio::task::JoinHandle<()>>,
    /// A byte came on the stdout.
    answered: bool,
}

impl ChildIo {
    /// Start `command` in the shell of the system. Call it in a tokio
    /// runtime: a task of the runtime reads the stderr.
    pub fn spawn(command: &str) -> io::Result<ChildIo> {
        ChildIo::start(shell(command))
    }

    fn start(command: std::process::Command) -> io::Result<ChildIo> {
        let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        let mut child = tokio::process::Command::from(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("the command has no stdout"))?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let drain = child.stderr.take().map(|pipe| runtime.spawn(keep_tail(pipe, stderr.clone())));
        Ok(ChildIo { child, stdin, stdout, stderr, drain, answered: false })
    }

    /// The exit status, when the command has ended.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// The last bytes of the stderr of the command that the reader took, at
    /// most [`STDERR_KEPT`].
    pub fn stderr_tail(&self) -> Vec<u8> {
        self.stderr.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// True when the command wrote a byte on its stdout.
    pub fn answered(&self) -> bool {
        self.answered
    }

    /// The last line of the kept stderr that is not blank, made safe to
    /// print.
    fn said(&self) -> String {
        let tail = self.stderr_tail();
        let text = String::from_utf8_lossy(&tail);
        text.lines().rev().map(pod_core::text::one_line).find(|l| !l.is_empty()).unwrap_or_default()
    }

    /// The exit status of the command when it ends by `deadline`, with its
    /// stderr read to the end (or to the deadline).
    async fn ended_by(&mut self, deadline: tokio::time::Instant) -> Option<ExitStatus> {
        // At most EXIT_WAIT: the command closed its stdout and is about to
        // end, or it holds on and the failure of the exchange stands.
        let deadline = deadline.min(tokio::time::Instant::now() + EXIT_WAIT);
        let status = tokio::time::timeout_at(deadline, self.child.wait()).await.ok()?.ok()?;
        // A child of the command can hold the stderr open.
        if let Some(drain) = self.drain.as_mut()
            && tokio::time::timeout_at(deadline, drain).await.is_ok()
        {
            self.drain = None;
        }
        Some(status)
    }
}

impl Drop for ChildIo {
    fn drop(&mut self) {
        if let Some(drain) = self.drain.take() {
            drain.abort();
        }
    }
}

/// Read `pipe` to its end, and keep its last [`STDERR_KEPT`] bytes in `tail`.
async fn keep_tail(mut pipe: tokio::process::ChildStderr, tail: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0u8; 1024];
    while let Ok(n @ 1..) = pipe.read(&mut chunk).await {
        keep_last(&mut tail.lock().unwrap_or_else(|e| e.into_inner()), &chunk[..n]);
    }
}

/// Add `bytes` to `tail`, and drop its oldest bytes past [`STDERR_KEPT`].
fn keep_last(tail: &mut Vec<u8>, bytes: &[u8]) {
    tail.extend_from_slice(bytes);
    let over = tail.len().saturating_sub(STDERR_KEPT);
    tail.drain(..over);
}

/// The shell of the system with the command line of the user.
#[cfg(not(windows))]
fn shell(command: &str) -> std::process::Command {
    let mut shell = std::process::Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}

/// The shell of the system with the command line of the user. The line goes
/// to `cmd.exe` as it is: an argument with quotes would get escapes that
/// `cmd.exe` does not remove. `/S` removes only the outer quotes, and `/D`
/// stops the AutoRun commands of the registry.
#[cfg(windows)]
fn shell(command: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut shell = std::process::Command::new("cmd.exe");
    shell.raw_arg(format!("/D /S /C \"{command}\""));
    shell
}

/// The status of an exit in words, and true when the shell could not run
/// the command or did not find it: another start gives the same.
fn status_text(status: ExitStatus) -> (String, bool) {
    match status.code() {
        Some(126) => ("126 (cannot run)".into(), true),
        Some(127) => ("127 (not found)".into(), true),
        // cmd.exe: "is not recognized as an internal or external command".
        Some(9009) if cfg!(windows) => ("9009 (not found)".into(), true),
        Some(code) => (code.to_string(), false),
        None => (signal_text(status), false),
    }
}

#[cfg(unix)]
fn signal_text(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map_or_else(|| "with no code".into(), |signal| format!("by signal {signal}"))
}

#[cfg(not(unix))]
fn signal_text(_: ExitStatus) -> String {
    "with no code".into()
}

/// The failures of the commands that exited 126 or 127, by command line,
/// each until the end of its hold.
static HELD: LazyLock<Mutex<HashMap<String, (Instant, DialError)>>> = LazyLock::new(Default::default);

/// The failure of `command` when it is in its hold.
pub(crate) fn held(command: &str) -> Option<DialError> {
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    held.retain(|_, (until, _)| *until > now);
    held.get(command).map(|(_, failure)| failure.clone())
}

/// The failure of `error`, the error of an exchange on `stream` with
/// `proxy`: when the stream is a command that ended before it wrote a byte,
/// [`DialError::ProxyExited`], which a command that the shell cannot run or
/// does not find also holds for [`EXEC_HOLD`]; else `error`.
pub(crate) async fn or_exited(
    stream: &mut Stream,
    proxy: &Proxy,
    deadline: tokio::time::Instant,
    error: DialError,
) -> DialError {
    let (Stream::Exec(child), Carrier::Exec(command)) = (stream, proxy.carrier()) else { return error };
    if child.answered {
        return error;
    }
    let Some(status) = child.ended_by(deadline).await else { return error };
    let (status, fatal) = status_text(status);
    let exited =
        DialError::ProxyExited { word: command_word(command).unwrap_or_default().into(), status, said: child.said() };
    if fatal {
        let until = Instant::now() + EXEC_HOLD;
        HELD.lock().unwrap_or_else(|e| e.into_inner()).insert(command.clone(), (until, exited.clone()));
    }
    exited
}

impl AsyncRead for ChildIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.stdout).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.answered = true;
        }
        polled
    }
}

impl AsyncWrite for ChildIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_write(cx, data),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_write_vectored(cx, bufs),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.stdin.as_ref().is_some_and(|stdin| stdin.is_write_vectored())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.stdin.as_mut() {
            Some(stdin) => Pin::new(stdin).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stdin = None;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_keeps_the_last_bytes_only() {
        let mut tail = Vec::new();
        keep_last(&mut tail, &[b'a'; 400]);
        keep_last(&mut tail, &[b'b'; 200]);
        assert_eq!(tail.len(), STDERR_KEPT);
        assert!(tail.starts_with(&[b'a'; 312]) && tail.ends_with(&[b'b'; 200]));
        keep_last(&mut tail, &[b'c'; 2000]);
        assert_eq!(tail, [b'c'; STDERR_KEPT]);
    }

    #[cfg(unix)]
    #[test]
    fn an_exit_of_the_shell_is_named() {
        use std::os::unix::process::ExitStatusExt;
        // The raw status of wait(2): the code is in the second byte.
        assert_eq!(status_text(ExitStatus::from_raw(127 << 8)), ("127 (not found)".into(), true));
        assert_eq!(status_text(ExitStatus::from_raw(126 << 8)), ("126 (cannot run)".into(), true));
        assert_eq!(status_text(ExitStatus::from_raw(3 << 8)), ("3".into(), false));
        assert_eq!(status_text(ExitStatus::from_raw(9)), ("by signal 9".into(), false));
    }
}
