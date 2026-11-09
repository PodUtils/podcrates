//! The answer of `__atomic_is_lock_free` on i686 with musl.
//!
//! OpenSSL (`crypto/threads_pthread.c`) asks the compiler's runtime whether
//! an atomic of a size needs a lock. On i686 the compiler cannot answer at
//! compile time and calls `__atomic_is_lock_free`, which zig's compiler-rt
//! does not have: the static link of a binary fails. This is the answer of
//! GCC's libatomic for i686: an atomic of 1, 2, 4 or 8 bytes needs no lock
//! (8 bytes with `cmpxchg8b`), and the compiler makes each of them with no
//! call.

/// True when an atomic of `size` bytes needs no lock.
#[unsafe(no_mangle)]
extern "C" fn __atomic_is_lock_free(size: usize, _ptr: *const core::ffi::c_void) -> bool {
    matches!(size, 1 | 2 | 4 | 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_of_i686_need_no_lock() {
        for size in [1, 2, 4, 8] {
            assert!(__atomic_is_lock_free(size, core::ptr::null()), "{size}");
        }
        assert!(!__atomic_is_lock_free(16, core::ptr::null()));
    }
}
