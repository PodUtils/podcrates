// ------------------------------------------------- stat, which has three names
//
// **The same syscall is called three different things by the kernel, and on
// two architectures it writes a different struct.** From the tables of the
// kernel:
//
//   newfstatat + struct stat     x86_64, riscv64, powerpc64, s390x
//   fstatat    + struct stat     aarch64, loongarch64
//   fstatat64  + struct stat64   arm, x86        (32-bit: `stat` is the narrow
//                                                 form and would truncate)
//
// The pairing is the point. Taking `fstatat64`'s number with `struct stat`'s
// buffer is a kernel write of the wrong shape into the right-sized hole, which
// is silent. The type and the number move together here so they cannot drift.

#[cfg(any(
    target_arch = "x86_64",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "powerpc64",
    target_arch = "s390x",
    target_arch = "sparc64",
    target_arch = "mips64",
))]
mod stat_call {
    pub use linux_raw_sys::general::stat as KernelStat;
    pub const SYS_FSTATAT: i64 = nr!(newfstatat, __NR_newfstatat);
}

#[cfg(any(target_arch = "aarch64", target_arch = "loongarch64", target_arch = "csky"))]
mod stat_call {
    pub use linux_raw_sys::general::stat as KernelStat;
    pub const SYS_FSTATAT: i64 = nr!(fstatat, __NR_fstatat);
}

#[cfg(any(
    target_arch = "arm",
    target_arch = "x86",
    target_arch = "powerpc",
    target_arch = "mips",
    target_arch = "sparc",
))]
mod stat_call {
    pub use linux_raw_sys::general::stat64 as KernelStat;
    pub const SYS_FSTATAT: i64 = nr!(fstatat64, __NR_fstatat64);
}

pub use stat_call::{KernelStat, SYS_FSTATAT};

/// The fallback of the stat call: loongarch64 before Linux 6.11 has no
/// `newfstatat`.
pub const SYS_STATX: i64 = nr!(statx, __NR_statx);

// -------------------------------------------- the three that ARE the question
//
// A probe measures the errno of a NAMED syscall, so for these
// three the entry point is not an implementation detail: it is what the row
// says was asked. On an architecture whose kernel has no `chown(2)` the honest
// answer is not to quietly call `fchownat(2)` and print `chown`, it is to call
// what exists and SAY which, which is what [`Entry`] carries into the report.

/// A syscall this probe set measures by name, and the entry point this
/// architecture actually has for it.
#[derive(Clone, Copy)]
pub struct Entry {
    pub nr: i64,
    /// The name the row prints. Always the name of the syscall that was
    /// really issued, never the one the probe set is called after.
    pub name: &'static str,
    /// True where this kernel has no dedicated entry point and the `*at` form
    /// is the only way to ask. The report says so rather than hiding it.
    pub via_at: bool,
}

/// **The one list in this crate that is a claim about the kernel and not a
/// value read from it**: the architectures taking their table from
/// `asm-generic/unistd.h`, which defines only the `*at` forms.
///
/// It is written exactly twice, positive and negated, on adjacent lines, and
/// the compiler checks it in both directions. Name an architecture here that
/// does have `chown` and nothing is lost but a more specific row; fail to name
/// one that does not and the build fails on `Sysno::chown`, which has no
/// variant there. Neither mistake can produce a wrong number at runtime, and
/// that is the property the old hand-written table did not have.
#[cfg(any(
    target_arch = "aarch64",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "loongarch64",
    target_arch = "csky",
))]
mod entry_points {
    use super::Entry;
    pub const CHOWN: Entry = Entry { nr: nr!(fchownat, __NR_fchownat), name: "fchownat", via_at: true };
    pub const LCHOWN: Entry =
        Entry { nr: nr!(fchownat, __NR_fchownat), name: "fchownat(AT_SYMLINK_NOFOLLOW)", via_at: true };
    pub const MKNOD: Entry = Entry { nr: nr!(mknodat, __NR_mknodat), name: "mknodat", via_at: true };
}

#[cfg(not(any(
    target_arch = "aarch64",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "loongarch64",
    target_arch = "csky",
)))]
mod entry_points {
    use super::Entry;
    // x86 and arm: plain `chown` takes 16-bit ids, so the `*32` form is the
    // one that can name an id above 65535.
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    pub const CHOWN: Entry = Entry { nr: nr!(chown32, __NR_chown32), name: "chown32", via_at: false };
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    pub const LCHOWN: Entry = Entry { nr: nr!(lchown32, __NR_lchown32), name: "lchown32", via_at: false };
    #[cfg(not(any(target_arch = "x86", target_arch = "arm")))]
    pub const CHOWN: Entry = Entry { nr: nr!(chown, __NR_chown), name: "chown", via_at: false };
    #[cfg(not(any(target_arch = "x86", target_arch = "arm")))]
    pub const LCHOWN: Entry = Entry { nr: nr!(lchown, __NR_lchown), name: "lchown", via_at: false };
    pub const MKNOD: Entry = Entry { nr: nr!(mknod, __NR_mknod), name: "mknod", via_at: false };
}

pub use entry_points::{CHOWN, LCHOWN, MKNOD};
