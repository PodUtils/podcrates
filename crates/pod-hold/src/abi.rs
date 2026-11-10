//! The kernel side of the seccomp user notification: the structures, the
//! numbers of the ioctls and the audit arch of the build target.
//!
//! Each ioctl number comes from its encoding (`_IOC`), not from a written
//! constant: powerpc and mips put the direction in three bits at bit 29,
//! with `_IOC_WRITE` 4, where the other architectures put it in two bits at
//! bit 30, with `_IOC_WRITE` 1. A written `_IOW` of x86_64 is a wrong number
//! on powerpc64le.

/// `struct seccomp_data`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SeccompData {
    pub nr: i32,
    pub arch: u32,
    pub instruction_pointer: u64,
    pub args: [u64; 6],
}

/// `struct seccomp_notif`: one stopped system call.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SeccompNotif {
    pub id: u64,
    pub pid: u32,
    pub flags: u32,
    pub data: SeccompData,
}

/// `struct seccomp_notif_resp`: the reply.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SeccompNotifResp {
    pub id: u64,
    pub val: i64,
    pub error: i32,
    pub flags: u32,
}

/// `struct seccomp_notif_addfd`: a descriptor for the task of a
/// notification.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SeccompNotifAddfd {
    pub id: u64,
    pub flags: u32,
    pub srcfd: u32,
    pub newfd: u32,
    pub newfd_flags: u32,
}

pub const SECCOMP_USER_NOTIF_FLAG_CONTINUE: u32 = 1;
/// `SECCOMP_ADDFD_FLAG_SETFD`: install the fd at the number in `newfd`, in
/// place of the fd that has that number.
pub const SECCOMP_ADDFD_FLAG_SETFD: u32 = 1;
/// `SECCOMP_ADDFD_FLAG_SEND` (Linux 5.14): add the fd and answer the call
/// with its number, in one step.
pub const SECCOMP_ADDFD_FLAG_SEND: u32 = 2;

/// The two encodings of an ioctl number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// x86, arm, aarch64, riscv, loongarch and the rest
    /// (`asm-generic/ioctl.h`).
    Generic,
    /// powerpc, mips and sparc: three direction bits at bit 29, 13 size bits.
    ThreeBit,
}

/// The direction of an ioctl, as the kernel sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// `_IOW`: the caller writes, the kernel reads.
    Write,
    /// `_IOWR`.
    ReadWrite,
}

impl Encoding {
    /// The encoding of the build target.
    pub const NATIVE: Encoding = if cfg!(any(
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "sparc64"
    )) {
        Encoding::ThreeBit
    } else {
        Encoding::Generic
    };

    /// `_IOC(dir, ty, nr, size)`.
    pub const fn ioc(self, dir: Dir, ty: u8, nr: u8, size: usize) -> u32 {
        let (read, write, shift) = match self {
            Encoding::Generic => (2u32, 1u32, 30),
            Encoding::ThreeBit => (2u32, 4u32, 29),
        };
        let dir = match dir {
            Dir::Write => write,
            Dir::ReadWrite => read | write,
        };
        (dir << shift) | ((size as u32) << 16) | ((ty as u32) << 8) | nr as u32
    }
}

/// `SECCOMP_IOC_MAGIC`.
const MAGIC: u8 = b'!';

/// `SECCOMP_IOCTL_NOTIF_RECV`: `_IOWR('!', 0, struct seccomp_notif)`.
pub const SECCOMP_IOCTL_NOTIF_RECV: u32 = Encoding::NATIVE.ioc(Dir::ReadWrite, MAGIC, 0, size_of::<SeccompNotif>());
/// `SECCOMP_IOCTL_NOTIF_SEND`: `_IOWR('!', 1, struct seccomp_notif_resp)`.
pub const SECCOMP_IOCTL_NOTIF_SEND: u32 = Encoding::NATIVE.ioc(Dir::ReadWrite, MAGIC, 1, size_of::<SeccompNotifResp>());
/// `SECCOMP_IOCTL_NOTIF_ID_VALID`: `_IOW('!', 2, __u64)`.
pub const SECCOMP_IOCTL_NOTIF_ID_VALID: u32 = Encoding::NATIVE.ioc(Dir::Write, MAGIC, 2, size_of::<u64>());
/// `SECCOMP_IOCTL_NOTIF_ADDFD`: `_IOW('!', 3, struct seccomp_notif_addfd)`.
pub const SECCOMP_IOCTL_NOTIF_ADDFD: u32 = Encoding::NATIVE.ioc(Dir::Write, MAGIC, 3, size_of::<SeccompNotifAddfd>());
/// `TUNSETIFF`: `_IOW('T', 202, int)`.
pub const TUNSETIFF: u32 = Encoding::NATIVE.ioc(Dir::Write, b'T', 202, size_of::<i32>());

/// accept(2). i386 has no direct accept system call (only accept4, and
/// accept through socketcall), so the entry is accept4 there.
#[cfg(not(target_arch = "x86"))]
pub const SYS_ACCEPT: libc::c_long = libc::SYS_accept;
#[cfg(target_arch = "x86")]
pub const SYS_ACCEPT: libc::c_long = libc::SYS_accept4;

/// The audit arch value of this build target.
#[cfg(target_arch = "x86_64")]
pub const AUDIT_ARCH_NATIVE: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
pub const AUDIT_ARCH_NATIVE: u32 = 0xC000_00B7;
#[cfg(target_arch = "arm")]
pub const AUDIT_ARCH_NATIVE: u32 = 0x4000_0028;
#[cfg(target_arch = "riscv64")]
pub const AUDIT_ARCH_NATIVE: u32 = 0xC000_00F3;
#[cfg(target_arch = "x86")]
pub const AUDIT_ARCH_NATIVE: u32 = 0x4000_0003;
#[cfg(target_arch = "loongarch64")]
pub const AUDIT_ARCH_NATIVE: u32 = 0xC000_0102;
#[cfg(all(target_arch = "powerpc64", target_endian = "little"))]
pub const AUDIT_ARCH_NATIVE: u32 = 0xC000_0015;
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv64",
    target_arch = "x86",
    target_arch = "loongarch64",
    all(target_arch = "powerpc64", target_endian = "little")
)))]
compile_error!("pod-hold: add the AUDIT_ARCH value for this target");

/// The x32 system-call bit on x86_64. Zero on other targets.
#[cfg(target_arch = "x86_64")]
pub const X32_SYSCALL_BIT: u32 = 0x4000_0000;
#[cfg(not(target_arch = "x86_64"))]
pub const X32_SYSCALL_BIT: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    // The sizes that the kernel encodes in the ioctl numbers.
    #[test]
    fn the_structures_have_the_sizes_of_the_kernel() {
        assert_eq!(size_of::<SeccompData>(), 64);
        assert_eq!(size_of::<SeccompNotif>(), 80);
        assert_eq!(size_of::<SeccompNotifResp>(), 24);
        assert_eq!(size_of::<SeccompNotifAddfd>(), 24);
        assert_eq!(std::mem::offset_of!(SeccompNotif, data), 16);
        assert_eq!(std::mem::offset_of!(SeccompData, args), 16);
    }

    // The generic numbers are the numbers of linux/seccomp.h on x86_64 and
    // aarch64; the numbers of powerpc have the direction at bit 29.
    #[test]
    fn each_ioctl_number_follows_the_encoding_of_its_architecture() {
        let g = Encoding::Generic;
        assert_eq!(g.ioc(Dir::ReadWrite, MAGIC, 0, 80), 0xc050_2100);
        assert_eq!(g.ioc(Dir::ReadWrite, MAGIC, 1, 24), 0xc018_2101);
        assert_eq!(g.ioc(Dir::Write, MAGIC, 2, 8), 0x4008_2102);
        assert_eq!(g.ioc(Dir::Write, MAGIC, 3, 24), 0x4018_2103);
        assert_eq!(g.ioc(Dir::Write, b'T', 202, 4), 0x4004_54ca);
        let p = Encoding::ThreeBit;
        assert_eq!(p.ioc(Dir::ReadWrite, MAGIC, 0, 80), 0xc050_2100);
        assert_eq!(p.ioc(Dir::Write, MAGIC, 2, 8), 0x8008_2102);
        assert_eq!(p.ioc(Dir::Write, MAGIC, 3, 24), 0x8018_2103);
        assert_eq!(p.ioc(Dir::Write, b'T', 202, 4), 0x8004_54ca);
        let native = Encoding::NATIVE;
        assert_eq!(SECCOMP_IOCTL_NOTIF_ADDFD, native.ioc(Dir::Write, MAGIC, 3, 24));
        assert_eq!(TUNSETIFF, native.ioc(Dir::Write, b'T', 202, 4));
    }
}
