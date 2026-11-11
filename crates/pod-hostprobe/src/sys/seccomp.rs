//! The parts of a seccomp filter that each filter of the workspace shares:
//! the audit arch of the build target, the x32 bit, the opcodes of classic
//! BPF, the offsets of `struct seccomp_data`, and the install of a filter.
//! `pod-hold` (a filter of user notifications) and `pod-confine` (a deny
//! filter) take the architecture from here, so there is one table.

use super::*;

use linux_raw_sys::ptrace as k;

/// `struct sock_filter` and `struct sock_fprog` of the kernel of the build
/// target. The pointer of a `sock_fprog` is at the alignment of a pointer:
/// 8 on a 64-bit target, 4 on a 32-bit target.
pub use linux_raw_sys::ptrace::{sock_filter as SockFilter, sock_fprog as SockFprog};

/// The audit arch value of the build target: the `arch` of each
/// `seccomp_data`. A call of another ABI of the same kernel (i386 on
/// x86_64) has another value and other numbers.
#[cfg(target_arch = "x86_64")]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_X86_64;
#[cfg(target_arch = "x86")]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_I386;
#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_AARCH64;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_ARM;
#[cfg(all(target_arch = "arm", target_endian = "big"))]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_ARMEB;
#[cfg(target_arch = "riscv64")]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_RISCV64;
#[cfg(target_arch = "loongarch64")]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_LOONGARCH64;
#[cfg(all(target_arch = "powerpc64", target_endian = "little"))]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_PPC64LE;
#[cfg(all(target_arch = "powerpc64", target_endian = "big"))]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_PPC64;
#[cfg(target_arch = "s390x")]
pub const AUDIT_ARCH_NATIVE: u32 = k::AUDIT_ARCH_S390X;
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "x86",
    all(target_arch = "aarch64", target_endian = "little"),
    target_arch = "arm",
    target_arch = "riscv64",
    target_arch = "loongarch64",
    target_arch = "powerpc64",
    target_arch = "s390x",
)))]
compile_error!("pod-hostprobe: add the AUDIT_ARCH value of this target to sys/seccomp.rs");

/// The x32 bit of a call number on x86_64: an x32 call has the arch value
/// of x86_64 and its number with this bit set. Zero on the other targets.
#[cfg(target_arch = "x86_64")]
pub const X32_SYSCALL_BIT: u32 = linux_raw_sys::general::__X32_SYSCALL_BIT;
#[cfg(not(target_arch = "x86_64"))]
pub const X32_SYSCALL_BIT: u32 = 0;

/// The opcodes of classic BPF that a seccomp filter uses
/// (`linux/bpf_common.h`; the same on each architecture).
/// `BPF_LD | BPF_W | BPF_ABS`: load the word at an offset of the data.
pub const BPF_LD_W_ABS: u16 = 0x20;
/// `BPF_JMP | BPF_JEQ | BPF_K`.
pub const BPF_JEQ_K: u16 = 0x15;
/// `BPF_JMP | BPF_JGE | BPF_K`.
pub const BPF_JGE_K: u16 = 0x35;
/// `BPF_JMP | BPF_JSET | BPF_K`: jump when `A & k` is not zero.
pub const BPF_JSET_K: u16 = 0x45;
/// `BPF_RET | BPF_K`.
pub const BPF_RET_K: u16 = 0x06;
/// The most instructions of one program (`BPF_MAXINSNS`).
pub const BPF_MAXINSNS: usize = 4096;

pub const SECCOMP_RET_ALLOW: u32 = k::SECCOMP_RET_ALLOW;
pub const SECCOMP_RET_ERRNO: u32 = k::SECCOMP_RET_ERRNO;
/// The most errno that `SECCOMP_RET_ERRNO` gives (`MAX_ERRNO`).
pub const SECCOMP_MAX_ERRNO: u32 = 4095;

/// The offsets of `struct seccomp_data`: the number, then the arch.
pub const SECCOMP_DATA_NR: u32 = 0;
pub const SECCOMP_DATA_ARCH: u32 = 4;

/// The offset of the low 32-bit word of argument `index` (0 to 5) in
/// `struct seccomp_data`. Each argument is a `u64` from offset 16; a
/// big-endian target keeps the low word at the higher address.
pub const fn seccomp_arg_low(index: u32) -> u32 {
    16 + 8 * index + if cfg!(target_endian = "big") { 4 } else { 0 }
}

/// The index of the flags argument of `clone(2)`: the second on s390x
/// (`CLONE_BACKWARDS2`), the first on the other targets.
pub const CLONE_FLAGS_ARG: u32 = if cfg!(target_arch = "s390x") { 1 } else { 0 };

/// One instruction with no jump.
pub const fn bpf_stmt(code: u16, k: u32) -> SockFilter {
    SockFilter { code, jt: 0, jf: 0, k }
}

/// One conditional jump: `jt` instructions on true, `jf` on false.
pub const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code, jt, jf, k }
}

/// `seccomp(SECCOMP_SET_MODE_FILTER, flags, prog)`: install the program on
/// this thread. One system call on the program of the caller, so a child
/// between its fork and its exec can call it. An unprivileged thread needs
/// `no_new_privs` first.
pub fn seccomp_set_filter(filter: &[SockFilter], flags: u64) -> Sysres {
    if filter.is_empty() || filter.len() > BPF_MAXINSNS {
        return Err(EINVAL);
    }
    let prog = SockFprog { len: filter.len() as u16, filter: filter.as_ptr() as *mut SockFilter };
    // SAFETY: `prog` points to `filter`, which lives until the call
    // returns; the kernel copies the program and does not write to it.
    unsafe { sys(SYS_SECCOMP, [SECCOMP_SET_MODE_FILTER, flags, &prog as *const SockFprog as u64, 0, 0, 0]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The kernel reads the pointer of `struct sock_fprog` at the size of a
    // pointer, on a 32-bit target too.
    #[test]
    fn the_filter_pointer_is_where_the_kernel_reads_it() {
        assert_eq!(std::mem::offset_of!(SockFprog, filter), size_of::<usize>());
        assert_eq!(size_of::<SockFprog>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<SockFilter>(), 8);
    }

    // The arch values of the targets of section 7 of docs/design.md, as
    // linux/audit.h composes them: EM_* with the 64-bit and the
    // little-endian bits.
    #[test]
    fn the_arch_value_is_the_one_of_audit_h() {
        let expected = if cfg!(target_arch = "x86_64") {
            0xC000_003E
        } else if cfg!(target_arch = "aarch64") {
            0xC000_00B7
        } else if cfg!(target_arch = "x86") {
            0x4000_0003
        } else if cfg!(target_arch = "arm") {
            0x4000_0028
        } else if cfg!(target_arch = "riscv64") {
            0xC000_00F3
        } else if cfg!(target_arch = "loongarch64") {
            0xC000_0102
        } else {
            AUDIT_ARCH_NATIVE
        };
        assert_eq!(AUDIT_ARCH_NATIVE, expected);
        assert_eq!(X32_SYSCALL_BIT, if cfg!(target_arch = "x86_64") { 0x4000_0000 } else { 0 });
        assert_eq!(seccomp_arg_low(0), if cfg!(target_endian = "big") { 20 } else { 16 });
    }

    #[test]
    fn an_empty_or_long_program_is_refused_before_the_call() {
        assert_eq!(seccomp_set_filter(&[], 0), Err(EINVAL));
        let long = vec![bpf_stmt(BPF_RET_K, SECCOMP_RET_ALLOW); BPF_MAXINSNS + 1];
        assert_eq!(seccomp_set_filter(&long, 0), Err(EINVAL));
    }
}
