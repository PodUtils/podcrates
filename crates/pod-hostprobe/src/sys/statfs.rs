//! `statfs(2)` and `fstatfs(2)`: the size, the type and the mount flags of
//! a file system.

use super::*;

/// The `statfs` fields this project reads, widened to one shape.
///
/// **The kernel's own `struct statfs64` is the buffer**, taken per
/// architecture from `linux_raw_sys`, and this is a normalised view built from
/// it. The two are different things on purpose: the kernel's layout and field
/// widths differ by architecture, and a hand-written `#[repr(C)]` that is one
/// field short is a kernel write past the end of it.
#[derive(Default, Clone, Copy)]
pub struct Statfs {
    pub f_bsize: i64,
    /// The size of the file system, in blocks of `f_bsize`.
    pub f_blocks: u64,
    /// The free blocks for a user that is not root.
    pub f_bavail: u64,
    /// The free blocks, also those that the file system keeps for root
    /// (the reserved blocks of ext4): the room of a daemon of root.
    pub f_bfree: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    /// The mount flags (`ST_RDONLY`, `ST_NOSUID`, `ST_NODEV`, `ST_NOEXEC`,
    /// ...). The kernel sets `ST_VALID` beside them where it fills this
    /// field; a value without `ST_VALID` tells nothing.
    pub f_flags: u64,
    /// The magic number of the file system (`OVERLAYFS_SUPER_MAGIC`, ...).
    pub f_type: u64,
}

impl Statfs {
    /// True where the kernel filled `f_flags` and `flag` is set in it.
    /// `None` where the kernel did not fill the field (no `ST_VALID`).
    pub fn flag(&self, flag: u64) -> Option<bool> {
        (self.f_flags & ST_VALID != 0).then_some(self.f_flags & flag != 0)
    }
}

/// The mount flags of `Statfs::f_flags`, as the kernel defines them
/// (`include/linux/statfs.h`) on each architecture.
pub const ST_RDONLY: u64 = 0x0001;
pub const ST_NOSUID: u64 = 0x0002;
pub const ST_NODEV: u64 = 0x0004;
pub const ST_NOEXEC: u64 = 0x0008;
pub const ST_SYNCHRONOUS: u64 = 0x0010;
/// Set by the kernel where it filled `f_flags`.
pub const ST_VALID: u64 = 0x0020;
pub const ST_MANDLOCK: u64 = 0x0040;
pub const ST_NOATIME: u64 = 0x0400;
pub const ST_NODIRATIME: u64 = 0x0800;
pub const ST_RELATIME: u64 = 0x1000;

/// `ST_NOSYMFOLLOW`: no symbolic link of the mount is followed (Linux 5.10).
pub const ST_NOSYMFOLLOW: u64 = 0x2000;

pub fn statfs(path: &CBuf) -> Result<Statfs, Errno> {
    #[cfg(target_pointer_width = "64")]
    let call = SYS_STATFS;
    #[cfg(target_pointer_width = "32")]
    let call = crate::sys::SYS_STATFS64;
    statfs_call(call, path.ptr())
}

/// `fstatfs(2)` of a descriptor, also of an `O_PATH` one. The child of an
/// entry reads the flags of a mount with it after the fork: it takes no
/// path, so it allocates nothing.
pub fn fstatfs(fd: i64) -> Result<Statfs, Errno> {
    #[cfg(target_pointer_width = "64")]
    let call = SYS_FSTATFS;
    #[cfg(target_pointer_width = "32")]
    let call = crate::sys::SYS_FSTATFS64;
    statfs_call(call, fd as u64)
}

/// `statfs` or `fstatfs` on `first`: a path or a descriptor.
fn statfs_call(call: i64, first: u64) -> Result<Statfs, Errno> {
    // `statfs64` and not `statfs`: on a 32-bit architecture the narrow form
    // truncates a filesystem larger than 4 GiB of blocks, and a free-space
    // check that silently wraps is worse than none. The syscall takes the
    // buffer's size so the kernel refuses a shape it does not know.
    let mut raw = unsafe { core::mem::zeroed::<linux_raw_sys::general::statfs64>() };
    let buf = &mut raw as *mut linux_raw_sys::general::statfs64 as u64;
    // A 64-bit kernel has one form, whose struct is the wide one.
    #[cfg(target_pointer_width = "64")]
    unsafe {
        sys(call, [first, buf, 0, 0, 0, 0])?
    };
    #[cfg(target_pointer_width = "32")]
    unsafe {
        let size = core::mem::size_of::<linux_raw_sys::general::statfs64>() as u64;
        sys(call, [first, size, buf, 0, 0, 0])?
    };
    // As `Stat::from_kernel`: redundant here, load-bearing elsewhere.
    #[allow(clippy::unnecessary_cast)]
    Ok(Statfs {
        f_bsize: raw.f_bsize as i64,
        f_blocks: raw.f_blocks as u64,
        f_bavail: raw.f_bavail as u64,
        f_bfree: raw.f_bfree as u64,
        f_files: raw.f_files as u64,
        f_ffree: raw.f_ffree as u64,
        // A signed `long` on most 64-bit targets, a `u32` on the 32-bit
        // ones and on s390x: the flags are small and positive on each.
        f_flags: raw.f_flags as u64,
        // A magic is 32 bits; a signed 32-bit `long` would sign-extend it.
        f_type: (raw.f_type as u64) & 0xffff_ffff,
    })
}
