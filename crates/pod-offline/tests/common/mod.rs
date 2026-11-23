//! Shared helpers of the tests: a scratch directory, and a pseudo-terminal
//! pair that skips with a named reason when the host has no `/dev/ptmx`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(1);

/// A directory under the temporary directory, removed on drop.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pod-offline-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Scratch { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A pseudo-terminal pair: the master side, and the path of the slave.
#[cfg(unix)]
pub struct Pty {
    pub master: std::fs::File,
    pub slave: PathBuf,
}

/// Open a pseudo-terminal pair, or give `None` with the reason that the
/// host has none. A test that gets `None` returns: a skip, not a failure.
#[cfg(unix)]
pub fn pty() -> Option<Pty> {
    use std::os::fd::FromRawFd;

    if !Path::new("/dev/ptmx").exists() {
        eprintln!("skip: /dev/ptmx is unavailable on this host");
        return None;
    }
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    // SAFETY: openpty writes two descriptors into the integers passed, with
    // no terminal settings of its own.
    let opened =
        unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
    if opened != 0 {
        let e = std::io::Error::last_os_error();
        eprintln!("skip: /dev/ptmx gave no pseudo-terminal: {e}");
        return None;
    }
    let mut name = [0 as libc::c_char; 256];
    // SAFETY: ttyname_r writes into the buffer of this function.
    let named = unsafe { libc::ttyname_r(slave, name.as_mut_ptr(), name.len()) };
    if named != 0 {
        // SAFETY: the two descriptors are open and this function owns them.
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        eprintln!("skip: the slave of /dev/ptmx has no name (errno {named})");
        return None;
    }
    // SAFETY: the name is a NUL-terminated string that ttyname_r wrote.
    let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    // SAFETY: this function owns the slave descriptor and gives it up here;
    // the serial line opens the path again.
    unsafe { libc::close(slave) };
    // SAFETY: the master descriptor is open, and the File owns it now.
    Some(Pty { master: unsafe { std::fs::File::from_raw_fd(master) }, slave: PathBuf::from(path) })
}

/// Read what the other side wrote, within `within`. A read of at least one
/// byte returns all of the bytes that are there.
#[cfg(unix)]
pub fn read_within(file: &mut std::fs::File, within: std::time::Duration) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::fd::AsRawFd;

    let deadline = std::time::Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no byte within the time"));
        }
        let mut poll = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: poll reads one pollfd of this function.
        let ready = unsafe { libc::poll(&mut poll, 1, ms) };
        if ready < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if ready == 0 {
            continue;
        }
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the other side ended"));
        }
        return Ok(buf[..n].to_vec());
    }
}
