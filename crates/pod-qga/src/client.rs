//! The client: one connection to the agent, the sync, and the typed calls.
//!
//! **The sync.** The socket can hold old answers: of a client before this
//! one, or of a call that ended at its bound. [`Client::sync`] writes the
//! byte 0xFF (the agent resets its parser on it) and then
//! `guest-sync-delimited` with a new id. The agent writes 0xFF before that
//! answer. The client drops each byte up to the 0xFF and reads answers until
//! one gives its id. After a call that did not get its answer (the bound, a
//! broken answer), the next call syncs again first.
//!
//! **One call at a time.** The agent answers in order and gives no id to an
//! answer. Each method takes `&mut self`.

mod line;

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pod_core::entropy::{Entropy, OsEntropy};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result, far_text};
use crate::wire::{self, Answer, Chunk, Exec, ExecStatus, OpenMode, SENTINEL, Shutdown};
use line::{Line, Stop};

/// The settings of a client. [`Settings::new`] gives the defaults.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The Unix socket of the agent: the host end of the virtio-serial
    /// channel `org.qemu.guest_agent.0`.
    pub socket: PathBuf,
    /// How long [`Client::connect`] waits for the socket. The emulator makes
    /// it when it starts. Default: no wait.
    pub connect_wait: Duration,
    /// The bound of one call, from the write of the request to the end of
    /// the answer. Default: 10 s.
    pub call_timeout: Duration,
    /// The longest answer, in bytes. Default: 64 MiB.
    pub max_answer: usize,
    /// The bytes of one `guest-file-read` and one `guest-file-write` of
    /// [`Client::read_file`] and [`Client::write_file`]. Its base64 must
    /// fit in `max_answer`. Default: 1 MiB.
    pub chunk: usize,
    /// How long [`Client::shutdown`] waits for a refusal. Default: 1 s.
    pub shutdown_wait: Duration,
}

impl Settings {
    pub fn new(socket: impl Into<PathBuf>) -> Settings {
        Settings {
            socket: socket.into(),
            connect_wait: Duration::ZERO,
            call_timeout: Duration::from_secs(10),
            max_answer: 64 << 20,
            chunk: 1 << 20,
            shutdown_wait: Duration::from_secs(1),
        }
    }
}

/// The largest id of a sync: an integer that each JSON reader keeps exact.
const ID_MASK: u64 = (1 << 53) - 1;

/// One connection to the agent.
pub struct Client {
    settings: Settings,
    line: Line,
    synced: bool,
    next_id: u64,
}

/// The first id of the syncs of a client. It only tells a new answer from
/// an old one, and is no secret; with no random bytes the clock gives it.
fn first_id() -> u64 {
    let mut b = [0u8; 8];
    let seed = match OsEntropy.fill(&mut b) {
        Ok(()) => u64::from_le_bytes(b),
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64 ^ u64::from(std::process::id())),
    };
    (seed & ID_MASK).max(1)
}

impl Client {
    /// Connect to the socket of the settings, waiting up to
    /// [`Settings::connect_wait`] for it, then sync.
    pub fn connect(settings: Settings) -> Result<Client> {
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&settings.socket) {
                Ok(s) => break s,
                Err(cause) => {
                    let spent = start.elapsed();
                    if spent >= settings.connect_wait {
                        return Err(Error::new(
                            "connect",
                            ErrorKind::NoAgent { socket: settings.socket.clone(), cause },
                        ));
                    }
                    std::thread::sleep((settings.connect_wait - spent).min(Duration::from_millis(50)));
                }
            }
        };
        let mut c = Client { settings, line: Line::new(stream), synced: false, next_id: first_id() };
        c.sync()?;
        Ok(c)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The error of a read or a write that stopped. The stream is no longer
    /// in step with the answers, so the next call syncs first.
    fn stopped(&mut self, call: &str, stop: Stop) -> Error {
        self.synced = false;
        let kind = match stop {
            Stop::Timeout => ErrorKind::Timeout { after: self.settings.call_timeout },
            Stop::Closed => ErrorKind::Closed,
            Stop::TooLong(n) => ErrorKind::Protocol(format!("an answer over {n} bytes")),
            Stop::Io(e) => ErrorKind::Io(e),
        };
        Error::new(call, kind)
    }

    /// Reset the parser of the agent, and drop each old answer.
    pub fn sync(&mut self) -> Result<()> {
        const CALL: &str = "guest-sync-delimited";
        self.synced = false;
        self.line.clear();
        let id = self.next_id;
        self.next_id = ((id + 1) & ID_MASK).max(1);
        let deadline = Instant::now() + self.settings.call_timeout;
        let mut req = vec![SENTINEL];
        req.extend_from_slice(wire::request(CALL, Some(json!({ "id": id }))).as_bytes());
        req.push(b'\n');
        if let Err(s) = self.line.send(&req, deadline) {
            return Err(self.stopped(CALL, s));
        }
        loop {
            let got = self
                .line
                .skip_past(SENTINEL, deadline)
                .and_then(|()| self.line.read_until(b'\n', deadline, self.settings.max_answer));
            let bytes = match got {
                Ok(b) => b,
                Err(s) => return Err(self.stopped(CALL, s)),
            };
            match wire::answer(&bytes) {
                Ok(Answer::Return(v)) if wire::sync_id(&v).ok() == Some(id) => {
                    self.synced = true;
                    return Ok(());
                }
                Ok(Answer::Error { class, desc }) => return Err(agent(CALL, &class, &desc)),
                // The answer of an older sync, or bytes that are not one:
                // the next 0xFF.
                Ok(Answer::Return(_)) | Err(_) => {}
            }
        }
    }

    /// One call: the request, then its answer within the bound.
    fn call(&mut self, execute: &str, arguments: Option<Value>) -> Result<Value> {
        if !self.synced {
            self.sync()?;
        }
        let deadline = Instant::now() + self.settings.call_timeout;
        let mut req = wire::request(execute, arguments).into_bytes();
        req.push(b'\n');
        let got = self
            .line
            .send(&req, deadline)
            .and_then(|()| self.line.read_until(b'\n', deadline, self.settings.max_answer));
        let bytes = match got {
            Ok(b) => b,
            Err(s) => return Err(self.stopped(execute, s)),
        };
        match wire::answer(&bytes) {
            Ok(Answer::Return(v)) => Ok(v),
            Ok(Answer::Error { class, desc }) => Err(agent(execute, &class, &desc)),
            Err(why) => {
                self.synced = false;
                Err(Error::protocol(execute, why))
            }
        }
    }

    /// `guest-ping`: the agent runs.
    pub fn ping(&mut self) -> Result<()> {
        self.call("guest-ping", None).map(drop)
    }

    /// `guest-exec`: start a command, and give its pid in the guest.
    pub fn exec(&mut self, e: &Exec) -> Result<i64> {
        const CALL: &str = "guest-exec";
        if e.path.is_empty() {
            return Err(Error::new(CALL, ErrorKind::Usage("a command with no path".into())));
        }
        let ret = self.call(CALL, Some(e.arguments()))?;
        wire::exec_pid(&ret).map_err(|why| Error::protocol(CALL, why))
    }

    /// `guest-exec-status`: the state of a command that [`Client::exec`]
    /// started. The agent forgets a command after it gives its end.
    pub fn exec_status(&mut self, pid: i64) -> Result<ExecStatus> {
        const CALL: &str = "guest-exec-status";
        let ret = self.call(CALL, Some(json!({ "pid": pid })))?;
        ExecStatus::from_return(&ret).map_err(|why| Error::protocol(CALL, why))
    }

    /// Ask for the status each `poll` until the command ends, for at most
    /// `limit`. The agent has no wait of its own.
    pub fn wait_exec(&mut self, pid: i64, poll: Duration, limit: Duration) -> Result<ExecStatus> {
        let deadline = Instant::now() + limit;
        loop {
            let status = self.exec_status(pid)?;
            if status.exited {
                return Ok(status);
            }
            if Instant::now() + poll > deadline {
                return Err(Error::new("guest-exec-status", ErrorKind::NotEnded { pid, after: limit }));
            }
            std::thread::sleep(poll);
        }
    }

    /// `guest-file-open`: a handle of the agent.
    pub fn file_open(&mut self, path: &str, mode: OpenMode) -> Result<i64> {
        const CALL: &str = "guest-file-open";
        if path.is_empty() {
            return Err(Error::new(CALL, ErrorKind::Usage("a file with no path".into())));
        }
        let ret = self.call(CALL, Some(json!({ "path": path, "mode": mode.as_str() })))?;
        wire::file_handle(&ret).map_err(|why| Error::protocol(CALL, why))
    }

    /// `guest-file-read`: at most `count` bytes.
    pub fn file_read(&mut self, handle: i64, count: usize) -> Result<Chunk> {
        const CALL: &str = "guest-file-read";
        if count == 0 {
            return Err(Error::new(CALL, ErrorKind::Usage("a read of no byte".into())));
        }
        let ret = self.call(CALL, Some(json!({ "handle": handle, "count": count })))?;
        let chunk = Chunk::from_return(&ret).map_err(|why| Error::protocol(CALL, why))?;
        if chunk.data.len() > count {
            return Err(Error::protocol(CALL, format!("{} bytes for a read of {count}", chunk.data.len())));
        }
        Ok(chunk)
    }

    /// `guest-file-write`: the count of the bytes that the agent wrote.
    pub fn file_write(&mut self, handle: i64, data: &[u8]) -> Result<u64> {
        const CALL: &str = "guest-file-write";
        let args = json!({ "handle": handle, "buf-b64": wire::base64(data), "count": data.len() });
        let ret = self.call(CALL, Some(args))?;
        let n = wire::written(&ret).map_err(|why| Error::protocol(CALL, why))?;
        if n > data.len() as u64 {
            return Err(Error::protocol(CALL, format!("{n} bytes written of {}", data.len())));
        }
        Ok(n)
    }

    /// `guest-file-close`.
    pub fn file_close(&mut self, handle: i64) -> Result<()> {
        self.call("guest-file-close", Some(json!({ "handle": handle }))).map(drop)
    }

    /// The bytes of the file at `path`, at most `limit` of them. The handle
    /// is closed also when a read fails.
    pub fn read_file(&mut self, path: &str, limit: u64) -> Result<Vec<u8>> {
        let handle = self.file_open(path, OpenMode::Read)?;
        let read = self.read_all(handle, path, limit);
        let closed = self.file_close(handle);
        let data = read?;
        closed?;
        Ok(data)
    }

    fn read_all(&mut self, handle: i64, path: &str, limit: u64) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        loop {
            let chunk = self.file_read(handle, self.settings.chunk.max(1))?;
            if (data.len() + chunk.data.len()) as u64 > limit {
                let what = format!("the file {}", far_text(path));
                return Err(Error::new("guest-file-read", ErrorKind::Limit { what, limit }));
            }
            data.extend_from_slice(&chunk.data);
            // An empty read with no end flag is an end too: the agent of
            // Windows sets the flag only on a read of no byte.
            if chunk.eof || chunk.data.is_empty() {
                return Ok(data);
            }
        }
    }

    /// Write `data` to the file at `path`: made, or cut to zero first. The
    /// handle is closed also when a write fails.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let handle = self.file_open(path, OpenMode::Write)?;
        let wrote = self.write_all(handle, data);
        let closed = self.file_close(handle);
        wrote?;
        closed
    }

    fn write_all(&mut self, handle: i64, data: &[u8]) -> Result<()> {
        for part in data.chunks(self.settings.chunk.max(1)) {
            let mut at = 0;
            while at < part.len() {
                let n = self.file_write(handle, &part[at..])?;
                if n == 0 {
                    return Err(Error::protocol("guest-file-write", "the agent wrote no byte"));
                }
                at += n as usize;
            }
        }
        Ok(())
    }

    /// `guest-shutdown`. The agent gives no answer when it obeys: the guest
    /// stops. A refusal within [`Settings::shutdown_wait`] is an error; an
    /// end of the connection or no answer is success. The emulator tells if
    /// the guest stopped.
    pub fn shutdown(&mut self, mode: Shutdown) -> Result<()> {
        const CALL: &str = "guest-shutdown";
        if !self.synced {
            self.sync()?;
        }
        let mut req = wire::request(CALL, Some(json!({ "mode": mode.as_str() }))).into_bytes();
        req.push(b'\n');
        if let Err(s) = self.line.send(&req, Instant::now() + self.settings.call_timeout) {
            return Err(self.stopped(CALL, s));
        }
        // The guest goes down: a later call syncs first.
        self.synced = false;
        let deadline = Instant::now() + self.settings.shutdown_wait;
        match self.line.read_until(b'\n', deadline, self.settings.max_answer) {
            Ok(bytes) => match wire::answer(&bytes) {
                Ok(Answer::Error { class, desc }) => Err(agent(CALL, &class, &desc)),
                Ok(Answer::Return(_)) => Ok(()),
                Err(why) => Err(Error::protocol(CALL, why)),
            },
            Err(Stop::Timeout | Stop::Closed) => Ok(()),
            Err(s) => Err(self.stopped(CALL, s)),
        }
    }
}

/// The refusal of the agent, with its text cut: the guest is not trusted.
fn agent(call: &str, class: &str, desc: &str) -> Error {
    Error::new(call, ErrorKind::Agent { class: far_text(class), desc: far_text(desc) })
}
