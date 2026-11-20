//! The calls through the C library: `syscall(2)` gives the errno of the
//! kernel in `errno`, with no rule of its own.

use crate::{Calls, OpenHow, SYS_OPENAT2};
use core::ffi::{CStr, c_int, c_long};

mod ffi {
    use core::ffi::{c_int, c_long};

    unsafe extern "C" {
        pub fn syscall(n: c_long, ...) -> c_long;
        pub fn close(fd: c_int) -> c_int;
        // glibc and musl both export it.
        pub fn __errno_location() -> *mut c_int;
    }
}

/// [`Calls`] through `syscall(2)` and `close(2)` of the C library.
///
/// Not for an interposer that exports `syscall` itself: the call would
/// come back into the interposer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Libc;

impl Calls for Libc {
    fn openat2(&self, dirfd: c_int, path: &CStr, how: &OpenHow) -> Result<c_int, c_int> {
        // SAFETY: the NUL-ended path and the `open_how` live for the call,
        // and the size is the size of that struct.
        let r = unsafe {
            ffi::syscall(
                SYS_OPENAT2 as c_long,
                dirfd as c_long,
                path.as_ptr(),
                how as *const OpenHow,
                core::mem::size_of::<OpenHow>(),
            )
        };
        if r < 0 {
            // SAFETY: the C library gives the errno of this thread.
            return Err(unsafe { *ffi::__errno_location() });
        }
        Ok(r as c_int)
    }

    fn close(&self, fd: c_int) {
        // SAFETY: the descriptor is the walk's own; a failure of close
        // leaves nothing to undo.
        unsafe { ffi::close(fd) };
    }
}
