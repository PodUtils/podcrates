//! The Windows named pipe: the server end with an access list of the user,
//! SYSTEM and the Administrators alone and no remote client, the check of
//! the user of the peer process at the accept, and the connect of a
//! client, which checks the user of the server.
//!
//! A read of a pipe in blocking mode has no timeout of its own: a watch
//! thread of each stream cancels the read at its deadline (`CancelIoEx`).

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::{ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::IO::CancelIoEx;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
};

use crate::catalogue::Catalogue;
use crate::frame::Stream;
use crate::host::Peer;
use crate::server::{self, Incoming, Settings, Shared, Snapshot};
use crate::wire::{Code, Fault};

/// The SID of LocalSystem: a service of the system may serve a client.
const SYSTEM: &str = "S-1-5-18";
const BUFFER: u32 = 64 * 1024;

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(Some(0)).collect()
}

fn check_name(name: &str) -> io::Result<()> {
    if name.starts_with(r"\\.\pipe\") && name.len() > 9 {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, r"a pipe name starts with \\.\pipe\"))
    }
}

struct Watch {
    state: Mutex<(Option<Instant>, bool, bool)>,
    cv: Condvar,
}

/// One end of a named pipe, with the deadline of each read and write.
pub struct PipeStream {
    file: File,
    watch: Arc<Watch>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl PipeStream {
    fn new(file: File) -> PipeStream {
        let watch = Arc::new(Watch { state: Mutex::new((None, false, false)), cv: Condvar::new() });
        let (w, handle) = (watch.clone(), file.as_raw_handle() as usize);
        std::thread::spawn(move || {
            let Ok(mut s) = w.state.lock() else { return };
            loop {
                let (deadline, done, _) = *s;
                if done {
                    return;
                }
                s = match deadline {
                    None => w.cv.wait(s).unwrap_or_else(|e| e.into_inner()),
                    Some(d) if Instant::now() >= d => {
                        // SAFETY: the handle lives until `done`, which the
                        // drop of the stream sets before it closes it.
                        unsafe { CancelIoEx(handle as HANDLE, std::ptr::null()) };
                        *s = (None, false, true);
                        s
                    }
                    Some(d) => {
                        w.cv.wait_timeout(s, d - Instant::now()).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0)
                    }
                };
            }
        });
        PipeStream { file, watch, read_timeout: None, write_timeout: None }
    }

    fn timed<T>(&mut self, timeout: Option<Duration>, op: impl FnOnce(&mut File) -> io::Result<T>) -> io::Result<T> {
        if let Ok(mut s) = self.watch.state.lock() {
            *s = (timeout.map(|t| Instant::now() + t), false, false);
        }
        self.watch.cv.notify_all();
        let result = op(&mut self.file);
        let cancelled = self.watch.state.lock().map(|mut s| {
            let c = s.2;
            *s = (None, false, false);
            c
        });
        match result {
            Err(e) if cancelled.unwrap_or(false) || e.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) => {
                Err(io::ErrorKind::TimedOut.into())
            }
            other => other,
        }
    }
}

impl Drop for PipeStream {
    fn drop(&mut self) {
        if let Ok(mut s) = self.watch.state.lock() {
            s.1 = true;
        }
        self.watch.cv.notify_all();
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.timed(self.read_timeout, |f| f.read(buf)) {
            // The other end closed: the end of the stream.
            Err(e) if e.raw_os_error() == Some(109) => Ok(0),
            other => other,
        }
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.timed(self.write_timeout, |f| f.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Stream for PipeStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.read_timeout = timeout;
        Ok(())
    }

    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.write_timeout = timeout;
        Ok(())
    }
}

/// A new instance of the pipe `name`; the first one fails when another
/// server holds the name.
fn instance(name: &[u16], first: bool, attributes: &pod_core::files::PrivateAttributes) -> io::Result<File> {
    let open = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
    let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    // SAFETY: `name` ends with a NUL; the attributes live for the call.
    let h = unsafe {
        CreateNamedPipeW(name.as_ptr(), open, mode, PIPE_UNLIMITED_INSTANCES, BUFFER, BUFFER, 0, attributes.as_ptr())
    };
    if h == INVALID_HANDLE_VALUE {
        let e = io::Error::last_os_error();
        // ERROR_ACCESS_DENIED for a first instance: another server has the name.
        return Err(if first && e.raw_os_error() == Some(5) { io::Error::new(io::ErrorKind::AddrInUse, e) } else { e });
    }
    // SAFETY: a new handle that the file owns from here.
    Ok(unsafe { File::from_raw_handle(h as _) })
}

fn peer_of(file: &File) -> Peer {
    let mut pid = 0u32;
    // SAFETY: a pipe handle and a place for the id.
    let ok = unsafe { GetNamedPipeClientProcessId(file.as_raw_handle() as HANDLE, &mut pid) } != 0;
    let sid = ok.then(|| pod_core::files::sid_of_process(pid).ok()).flatten();
    Peer { uid: None, pid: ok.then_some(pid), sid }
}

/// A running server of a named pipe. Its drop stops the accept of new
/// connections.
pub struct Server {
    shared: Arc<Shared>,
    name: String,
}

impl Server {
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.clone()
    }

    /// The connections that hold a place now. A connection gives its place
    /// back when the server ends it, after the client reads its last
    /// answer: a caller that counts the places waits for this count.
    pub fn active(&self) -> usize {
        self.shared.active.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        // Wake the accept thread, so that it sees the flag and ends.
        let _ = std::fs::OpenOptions::new().read(true).write(true).open(&self.name);
    }
}

/// Serve the control on the named pipe `name` (`\\.\pipe\NAME`).
pub fn serve(name: &str, settings: Settings, catalogue: Catalogue) -> io::Result<(Server, Incoming)> {
    check_name(name)?;
    let attributes = pod_core::files::PrivateAttributes::new()?;
    let own = pod_core::files::user_sid().map_err(io::Error::other)?.to_string();
    let wname = wide(name);
    let first = instance(&wname, true, &attributes)?;
    let shared = Arc::new(Shared {
        settings,
        catalogue,
        snapshot: Snapshot::default(),
        active: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
    });
    let (tx, rx) = mpsc::channel();
    let accept = shared.clone();
    std::thread::spawn(move || {
        let mut next = Some(first);
        loop {
            let file = match next.take().map_or_else(|| instance(&wname, false, &attributes), Ok) {
                Ok(f) => f,
                Err(_) => return,
            };
            // SAFETY: a pipe handle; no overlapped I/O.
            let ok = unsafe { ConnectNamedPipe(file.as_raw_handle() as HANDLE, std::ptr::null_mut()) } != 0;
            if !ok && io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                continue;
            }
            if accept.closed.load(Ordering::SeqCst) {
                return;
            }
            let peer = peer_of(&file);
            let stream = PipeStream::new(file);
            let Some(tier) = server::admit(&accept.settings, &peer, None, Some(&own)) else {
                server::refuse(stream, Fault::new(Code::Refused, "the peer is another user"), &accept.settings);
                continue;
            };
            let Some(slot) = server::take_slot(&accept) else {
                server::refuse(stream, Fault::new(Code::Busy, "too many clients"), &accept.settings);
                continue;
            };
            let (shared, calls) = (accept.clone(), tx.clone());
            std::thread::spawn(move || {
                let _slot = slot;
                // An error of I/O ends the connection; the peer sees the close.
                let _ = server::serve_connection(stream, peer, tier, &shared, &calls);
            });
        }
    });
    Ok((Server { shared, name: name.to_string() }, Incoming(rx)))
}

/// Connect to the pipe `name`. The server must run as this user or as
/// SYSTEM: a pipe of another user is `PermissionDenied`.
pub fn connect(name: &str) -> io::Result<PipeStream> {
    check_name(name)?;
    let wname = wide(name);
    let mut tries = 0;
    let file = loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            Ok(f) => break f,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && tries < 5 => {
                tries += 1;
                // SAFETY: `wname` ends with a NUL.
                unsafe { WaitNamedPipeW(wname.as_ptr(), 2000) };
            }
            Err(e) => return Err(e),
        }
    };
    let mut pid = 0u32;
    // SAFETY: a pipe handle and a place for the id.
    if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let theirs = pod_core::files::sid_of_process(pid)?;
    let ours = pod_core::files::user_sid().map_err(io::Error::other)?;
    if theirs != ours && theirs != SYSTEM {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the pipe is of another user"));
    }
    Ok(PipeStream::new(file))
}
