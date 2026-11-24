//! The content of a regular file: its bytes in order, or the regions of a
//! GNU sparse file at their offsets ([`crate::sparse`]).

use std::io::Read;

use pod_hostprobe::sys;

use crate::error::{self, Result};
use crate::sparse::Layout;

/// Write the content of `r` through `fd`, the descriptor of the new file
/// `name`. The caller closes `fd`.
pub(super) fn write(fd: i64, r: &mut dyn Read, sparse: Option<&Layout>, name: &str) -> Result<()> {
    match sparse {
        Some(l) => regions(fd, r, l, name),
        None => dense(fd, r, name),
    }
}

fn dense(fd: i64, r: &mut dyn Read, name: &str) -> Result<()> {
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                let mut off = 0;
                while off < n {
                    match sys::write(fd, &buf[off..n]) {
                        Ok(w) if w > 0 => off += w as usize,
                        Ok(_) => {
                            return Err(error::sys(
                                format!("a write of {name:?} returned 0 with bytes left"),
                                sys::EIO,
                            ));
                        }
                        Err(e) if e == sys::EINTR => continue,
                        Err(e) => return Err(error::sys(format!("cannot write {name:?}"), e)),
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(error::io(e)),
        }
    }
}

/// Each region at its offset (`pwrite`), then the real size (`ftruncate`):
/// the bytes between the regions are never written, so they stay holes.
fn regions(fd: i64, r: &mut dyn Read, l: &Layout, name: &str) -> Result<()> {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::FileExt;
    // SAFETY: `fd` is an open descriptor for the whole call, and
    // `ManuallyDrop` does not close it: the caller does.
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd as i32) });
    let failed = |e: std::io::Error| {
        error::io(std::io::Error::new(e.kind(), format!("cannot write the sparse file {name:?}: {e}")))
    };
    let mut buf = vec![0u8; 128 * 1024];
    for region in &l.regions {
        let (mut at, mut left) = (region.offset, region.len);
        while left > 0 {
            let n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
            r.read_exact(&mut buf[..n]).map_err(error::io)?;
            file.write_all_at(&buf[..n], at).map_err(failed)?;
            at += n as u64;
            left -= n as u64;
        }
    }
    file.set_len(l.real_size).map_err(failed)
}
