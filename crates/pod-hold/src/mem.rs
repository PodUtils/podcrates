//! The memory of the task of a notification: a read through
//! `/proc/<pid>/mem`, a write through that file, else with
//! `process_vm_writev`. A memfd stands in for the memory in a test: its
//! offset is the address.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use pod_core::{Class, Failure};

use crate::failure;

/// How the supervisor may write the memory of a task. A host can refuse
/// the write open of `/proc/<pid>/mem` and allow `process_vm_writev`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// Open `/proc/<pid>/mem` for writing first.
    pub write_open: bool,
    /// Write with `process_vm_writev` where the file is read-only.
    pub vm_write: bool,
}

impl Default for Access {
    fn default() -> Self {
        Access { write_open: true, vm_write: true }
    }
}

impl Access {
    /// No write: the open is read-only and `process_vm_writev` is not used.
    /// A consumer's test of a host that refuses each write.
    pub const READ_ONLY: Access = Access { write_open: false, vm_write: false };
}

/// The memory of one task, open for one notification.
#[derive(Debug)]
pub struct Memory {
    fd: Option<OwnedFd>,
    pid: u32,
    writable: bool,
    vm_write: bool,
}

impl Memory {
    /// Open the memory of the task `pid`. Try read-write first, because some
    /// calls write a result into the task. Open it again for each
    /// notification: a descriptor opened before the `execve` of the task
    /// keeps the old address space and reads 0 bytes.
    pub fn open(pid: u32, access: Access) -> Memory {
        let Ok(path) = CString::new(format!("/proc/{pid}/mem")) else {
            return Memory { fd: None, pid, writable: false, vm_write: false };
        };
        if access.write_open {
            // SAFETY: a valid path; the flags are constants.
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if fd >= 0 {
                // SAFETY: a new descriptor that nothing else owns.
                let fd = Some(unsafe { OwnedFd::from_raw_fd(fd) });
                return Memory { fd, pid, writable: true, vm_write: access.vm_write };
            }
        }
        // SAFETY: as above.
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        // SAFETY: a new descriptor that nothing else owns.
        let fd = (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) });
        let writable = access.vm_write && process_vm_writable(pid);
        Memory { fd, pid, writable, vm_write: access.vm_write }
    }

    /// The memory of a descriptor that stands in for a task (a memfd in a
    /// test): no `process_vm_writev`.
    pub fn of_fd(fd: OwnedFd, writable: bool) -> Memory {
        Memory { fd: Some(fd), pid: 0, writable, vm_write: false }
    }

    /// The descriptor, or -1 when the open failed.
    pub fn raw(&self) -> RawFd {
        self.fd.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    /// The descriptor, given to the caller (-1 when the open failed). The
    /// caller closes it.
    pub fn into_raw_fd(mut self) -> RawFd {
        self.fd.take().map_or(-1, std::os::fd::IntoRawFd::into_raw_fd)
    }

    /// The task.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// True when the supervisor can write the memory of the task.
    pub fn writable(&self) -> bool {
        self.writable
    }

    /// Read `len` bytes at `addr`.
    pub fn read(&self, addr: u64, len: usize) -> Result<Vec<u8>, Failure> {
        read(self.raw(), addr, len)
    }

    /// Write `data` at `addr`.
    pub fn write(&self, addr: u64, data: &[u8]) -> Result<(), Failure> {
        let pid = if self.vm_write { self.pid } else { 0 };
        write_to(self.raw(), pid, addr, data)
    }
}

/// The file offset of `addr + off`: an address above `i64::MAX` is no
/// address of a task.
fn offset(addr: u64, off: usize) -> Result<i64, Failure> {
    addr.checked_add(off as u64)
        .and_then(|at| i64::try_from(at).ok())
        .ok_or_else(|| failure::errno(Class::Usage, "an address past the end of the memory of the task", libc::EFAULT))
}

/// Read a pointer-sized word (a pointer or a `size_t`) of the task at
/// `offset` of `bytes`. The supervisor and the task have one ABI: the
/// filter refuses each call of a foreign ABI. `None` past the end.
pub fn read_word(bytes: &[u8], offset: usize) -> Option<u64> {
    const WORD: usize = size_of::<usize>();
    let word = bytes.get(offset..offset.checked_add(WORD)?)?;
    Some(usize::from_ne_bytes(word.try_into().ok()?) as u64)
}

/// Read `len` bytes of the task at `addr` through `mem`.
pub fn read(mem: RawFd, addr: u64, len: usize) -> Result<Vec<u8>, Failure> {
    let mut buf = vec![0u8; len];
    let mut off = 0;
    while off < len {
        let at = offset(addr, off)?;
        // SAFETY: the buffer is valid for `len - off` bytes from `off`.
        let n = unsafe { libc::pread(mem, buf[off..].as_mut_ptr() as *mut _, len - off, at) };
        if n < 0 {
            return Err(failure::last_os("pread(/proc/PID/mem)"));
        }
        if n == 0 {
            return Err(failure::errno(Class::Usage, "pread(/proc/PID/mem): no memory at the address", libc::EFAULT));
        }
        off += n as usize;
    }
    Ok(buf)
}

/// Write `data` at `addr` of the task through `mem` (opened for writing),
/// else with `process_vm_writev` to `pid` (0: no `process_vm_writev`).
pub fn write_to(mem: RawFd, pid: u32, addr: u64, data: &[u8]) -> Result<(), Failure> {
    let mut off = 0;
    while off < data.len() {
        let at = offset(addr, off)?;
        let n = if mem >= 0 {
            // SAFETY: the buffer is valid for `data.len() - off` bytes.
            unsafe { libc::pwrite(mem, data[off..].as_ptr() as *const _, data.len() - off, at) }
        } else {
            -1
        };
        let n = if n > 0 {
            n
        } else if pid != 0 {
            let local = libc::iovec { iov_base: data[off..].as_ptr() as *mut _, iov_len: data.len() - off };
            let Ok(remote_at) = usize::try_from(at) else {
                return Err(failure::errno(
                    Class::Usage,
                    "an address past the address space of the task",
                    libc::EFAULT,
                ));
            };
            let remote = libc::iovec { iov_base: remote_at as *mut _, iov_len: data.len() - off };
            // SAFETY: the local buffer is valid for iov_len bytes; the kernel
            // checks the remote range in the other process.
            unsafe { libc::process_vm_writev(pid as i32, &local, 1, &remote, 1, 0) }
        } else {
            n
        };
        if n <= 0 {
            return Err(failure::last_os("a write to the memory of the task"));
        }
        off += n as usize;
    }
    Ok(())
}

/// Whether `process_vm_writev` to `pid` passes the permission check: a
/// write of one byte to address 1 (never mapped) gives EFAULT when the
/// check passes.
pub fn process_vm_writable(pid: u32) -> bool {
    let byte = [0u8; 1];
    let local = libc::iovec { iov_base: byte.as_ptr() as *mut _, iov_len: 1 };
    let remote = libc::iovec { iov_base: std::ptr::dangling_mut::<u8>() as *mut _, iov_len: 1 };
    // SAFETY: a one-byte local buffer; the remote address is never mapped.
    let n = unsafe { libc::process_vm_writev(pid as i32, &local, 1, &remote, 1, 0) };
    n >= 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EFAULT)
}

/// A memfd of `bytes` that stands in for the memory of a task: sealed, so
/// that a read or a write out of it fails as at an address with no map.
#[cfg(any(test, feature = "fuzzing"))]
pub fn memfd(bytes: &[u8]) -> Result<OwnedFd, Failure> {
    use std::io::Write;
    // SAFETY: a valid name; the flags are constants.
    let fd = unsafe { libc::memfd_create(c"pod-hold-memory".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(failure::last_os("memfd_create"));
    }
    // SAFETY: a new descriptor that nothing else owns.
    let mut file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    file.write_all(bytes).map_err(|e| failure::host(Class::Unavailable, format!("the memory image: {e}")))?;
    let seals = libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    // SAFETY: fcntl on an open descriptor.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(failure::last_os("fcntl(F_ADD_SEALS)"));
    }
    Ok(OwnedFd::from(file))
}

#[cfg(test)]
mod tests;
