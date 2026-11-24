//! The device of a Unix kernel: `/dev/tpmrm0`, the node of the resource
//! manager of the kernel (Linux 4.12 and later, FreeBSD), which keeps the
//! handles of each open file apart and flushes them when the file closes.
//! `/dev/tpm0` has no resource manager: a caller that opens it takes the
//! handles of each other program, so it opts in by its path.
//!
//! The kernel takes a whole command in one write and gives its whole
//! response in one read (`tpm-dev-common.c`); a write of more than
//! `TPM_BUFSIZE` (4096) bytes gives `E2BIG`. The file is not blocking: a
//! poll(2) bounds the wait of a command where the kernel runs it apart
//! from the write (Linux 5.0 and later); before that, the write waits for
//! the command.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;
use std::time::Instant;

use pod_core::{Class, Failure};

use super::bound::{Limits, broken, host, left, size_field, system};
use super::check_command;
use crate::transport::Transport;
use crate::wire::{HEADER, MAX_RESPONSE};

/// The device of a TPM, open for reads and writes as long as the caller
/// holds it. One command at a time, each bounded by [`Limits`]. After a
/// failure on the way (a time limit, a response out of its bounds) the file
/// is closed, so the kernel flushes the handles of this file, and each next
/// call fails with the class `Unavailable`.
pub struct Device {
    file: Option<File>,
    name: String,
    limits: Limits,
}

impl Device {
    /// Open the device at `path`, such as `/dev/tpmrm0`. A path that does
    /// not exist is no TPM (`Unavailable`); a refusal of the system is
    /// `Permission` (on most systems the group `tss` may open the device).
    /// A path that is not a character device or a FIFO is refused before
    /// it is opened: a socket of swtpm takes [`super::Socket`], and a FIFO
    /// is the stand-in of a test.
    pub fn open(path: &Path, limits: Limits) -> Result<Device, Failure> {
        let name = format!("the TPM device {}", path.display());
        let meta = std::fs::metadata(path).map_err(|e| refused(&name, e))?;
        let kind = meta.file_type();
        if kind.is_socket() {
            let why = format!("no TPM: {} is a socket, not a device; a socket of swtpm takes Socket", path.display());
            return Err(host(Class::Unavailable, why));
        }
        if !(kind.is_char_device() || kind.is_fifo()) {
            return Err(host(Class::Unavailable, format!("no TPM: {} is not a device", path.display())));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| refused(&name, e))?;
        Ok(Device { file: Some(file), name, limits })
    }

    /// A device that is open already: a descriptor that a helper with the
    /// right to open the device passed, or one end of a socket pair of a
    /// test. `name` names it in each failure. The descriptor is set not
    /// blocking.
    pub fn from_fd(fd: OwnedFd, name: &str, limits: Limits) -> Result<Device, Failure> {
        let raw = fd.as_raw_fd();
        // SAFETY: F_GETFL and F_SETFL on a descriptor that `fd` owns.
        let set = unsafe {
            let flags = libc::fcntl(raw, libc::F_GETFL);
            flags >= 0 && libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
        };
        if !set {
            return Err(system(format!("{name} cannot be set not blocking"), io::Error::last_os_error()));
        }
        Ok(Device { file: Some(File::from(fd)), name: name.to_string(), limits })
    }
}

/// The failure of an open: no device is no TPM.
fn refused(name: &str, e: io::Error) -> Failure {
    match e.kind() {
        io::ErrorKind::NotFound => system(format!("no TPM: {name} does not exist"), e),
        io::ErrorKind::PermissionDenied => {
            system(format!("this user may not open {name}; on most systems the group tss may"), e)
        }
        _ => system(format!("no TPM: {name} cannot be opened"), e),
    }
}

impl Transport for Device {
    fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
        check_command(command)?;
        let Some(file) = self.file.as_mut() else {
            return Err(broken(&self.name));
        };
        let until = Instant::now() + self.limits.command;
        let answer = exchange(file, command, until, &self.name, &self.limits);
        if answer.is_err() {
            // The drop closes the file.
            self.file = None;
        }
        answer
    }
}

fn exchange(file: &mut File, command: &[u8], until: Instant, name: &str, limits: &Limits) -> Result<Vec<u8>, Failure> {
    let written = loop {
        match file.write(command) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => wait(file, libc::POLLOUT, until, name, limits)?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            other => break other,
        }
    };
    match written {
        Ok(n) if n == command.len() => {}
        Ok(n) => {
            return Err(host(Class::Unavailable, format!("{name} took {n} bytes of a command of {}", command.len())));
        }
        Err(e) => return Err(system(format!("{name} did not take the command"), e)),
    }
    // One byte more than the bound: a response that fills it is too long.
    let mut buffer = vec![0u8; MAX_RESPONSE + 1];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Err(host(Class::Unavailable, format!("{name} closed and gave no response"))),
            Ok(n) => {
                whole(&buffer[..n], name)?;
                buffer.truncate(n);
                return Ok(buffer);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => wait(file, libc::POLLIN, until, name, limits)?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(system(format!("the response of {name} cannot be read"), e)),
        }
    }
}

/// A response of one read: at most the bound, at least its header, and as
/// long as its size field says.
fn whole(bytes: &[u8], name: &str) -> Result<(), Failure> {
    let why = if bytes.len() > MAX_RESPONSE {
        format!("{name} gave a response of more than {MAX_RESPONSE} bytes")
    } else if bytes.len() < HEADER {
        format!("{name} gave {} bytes, less than the header of a response", bytes.len())
    } else {
        match size_field(bytes) {
            Some(size) if usize::try_from(size).is_ok_and(|s| s == bytes.len()) => return Ok(()),
            size => format!("{name} gave {} bytes of a response that says {}", bytes.len(), size.unwrap_or(0)),
        }
    };
    Err(host(Class::Protocol, why))
}

/// Wait until `file` is ready for `events`, or the time runs out.
fn wait(file: &File, events: libc::c_short, until: Instant, name: &str, limits: &Limits) -> Result<(), Failure> {
    loop {
        let rest = left(until, name, limits)?;
        let mut poll = libc::pollfd { fd: file.as_raw_fd(), events, revents: 0 };
        let ms = libc::c_int::try_from(rest.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
        // SAFETY: one pollfd of this frame, for a descriptor that `file`
        // holds open.
        match unsafe { libc::poll(&mut poll, 1, ms) } {
            // The next turn finds the time that remains, or none.
            0 => {}
            n if n > 0 => return Ok(()),
            _ => {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    return Err(system(format!("{name} cannot be waited for"), e));
                }
            }
        }
    }
}
