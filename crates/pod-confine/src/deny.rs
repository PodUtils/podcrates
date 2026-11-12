//! The seccomp deny list and its BPF program.
//!
//! The program checks the audit arch first: a call of another ABI of the
//! same kernel (i386 or x32 on x86_64) has other numbers and would pass
//! the list, so it gets `ENOSYS`. Then each rule of the list: a rule with
//! no condition returns its errno for each call of its number; a rule with
//! conditions returns its errno where each condition holds, else the next
//! rule of the number runs. Each other call runs.

use pod_core::{Class, Failure};
use pod_hostprobe::sys as k;
use pod_hostprobe::sys::SockFilter;

use crate::failure;

/// The namespace flags of `clone(2)` and `unshare(2)`: `CLONE_NEWNS`,
/// `CLONE_NEWCGROUP`, `CLONE_NEWUTS`, `CLONE_NEWIPC`, `CLONE_NEWUSER`,
/// `CLONE_NEWPID`, `CLONE_NEWNET`. Each is in the low 32 bits.
pub const NAMESPACE_FLAGS: u32 = 0x7E02_0000;
/// `CLONE_NEWTIME`: `unshare` takes it; in `clone` the bit is part of the
/// exit signal.
pub const CLONE_NEWTIME: u32 = 0x80;

const EPERM: u16 = 1;
const EACCES: u16 = 13;
const ENOSYS: u16 = 38;
const ENOPROTOOPT: u16 = 92;
const EPROTONOSUPPORT: u16 = 93;
const ESOCKTNOSUPPORT: u16 = 94;
const EAFNOSUPPORT: u16 = 97;

/// One condition of a rule, on the low 32 bits of one argument of the
/// call (`index` 0 to 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// The argument has a bit of `mask`.
    AnyBit { index: u8, mask: u32 },
    /// The argument, masked with `mask`, is `value`.
    Equal { index: u8, mask: u32, value: u32 },
    /// The argument, masked with `mask`, is not `value`.
    NotEqual { index: u8, mask: u32, value: u32 },
}

impl Arg {
    /// The index of the argument.
    pub fn index(self) -> u8 {
        match self {
            Arg::AnyBit { index, .. } | Arg::Equal { index, .. } | Arg::NotEqual { index, .. } => index,
        }
    }
}

/// One rule: a call that fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deny {
    /// The name of the call, for a reader.
    pub name: String,
    /// The number of the call on the build target.
    pub nr: u32,
    /// The errno that the call gets (1 to 4095).
    pub errno: u16,
    /// The conditions of the rule. Empty: each call of the number fails.
    /// Else the call fails only where each condition holds, and the next
    /// rule of the number runs where one does not.
    pub when: Vec<Arg>,
}

impl Deny {
    /// The call `nr` (a `SYS_*` number of `pod_hostprobe::sys`) fails with
    /// `errno`.
    pub fn call(name: impl Into<String>, nr: i64, errno: u16) -> Deny {
        Deny { name: name.into(), nr: nr as u32, errno, when: Vec::new() }
    }

    /// Add a condition: argument `index` has a bit of `mask`.
    pub fn when_flags(mut self, index: u8, mask: u32) -> Deny {
        self.when.push(Arg::AnyBit { index, mask });
        self
    }

    /// Add a condition: argument `index`, masked with `mask`, is `value`.
    pub fn when_equal(mut self, index: u8, mask: u32, value: u32) -> Deny {
        self.when.push(Arg::Equal { index, mask, value });
        self
    }

    /// Add a condition: argument `index`, masked with `mask`, is not
    /// `value`.
    pub fn when_not_equal(mut self, index: u8, mask: u32, value: u32) -> Deny {
        self.when.push(Arg::NotEqual { index, mask, value });
        self
    }

    /// The default list: the namespace calls (`unshare` and `clone` with a
    /// namespace flag, `setns`; `clone3` gets `ENOSYS`, as a filter cannot
    /// read its flags behind a pointer, and the C library then uses
    /// `clone`), the mount calls (`mount`, `umount2`, `umount` where the
    /// kernel has it, `pivot_root`, `open_tree`, `open_tree_attr`,
    /// `move_mount`, `fsopen`, `fsconfig`, `fsmount`, `fspick`,
    /// `mount_setattr`), `ptrace`, `process_vm_readv`,
    /// `process_vm_writev`, `keyctl`, `add_key`, `request_key`, `bpf`,
    /// `perf_event_open`, `kexec_load`, `kexec_file_load` where the kernel
    /// has it, `init_module`, `finit_module`, `delete_module`,
    /// `open_by_handle_at`, `name_to_handle_at` and the io_uring calls
    /// ([`Deny::io_uring`]). Each other one gets `EPERM`.
    pub fn defaults() -> Vec<Deny> {
        let mut v = vec![
            Deny::call("unshare", k::SYS_UNSHARE, EPERM).when_flags(0, NAMESPACE_FLAGS | CLONE_NEWTIME),
            Deny::call("clone", k::SYS_CLONE, EPERM).when_flags(k::CLONE_FLAGS_ARG as u8, NAMESPACE_FLAGS),
            Deny::call("clone3", k::SYS_CLONE3, ENOSYS),
            Deny::call("setns", k::SYS_SETNS, EPERM),
            Deny::call("mount", k::SYS_MOUNT, EPERM),
            Deny::call("umount2", k::SYS_UMOUNT2, EPERM),
            Deny::call("pivot_root", k::SYS_PIVOT_ROOT, EPERM),
            Deny::call("open_tree", k::SYS_OPEN_TREE, EPERM),
            Deny::call("open_tree_attr", k::SYS_OPEN_TREE_ATTR, EPERM),
            Deny::call("move_mount", k::SYS_MOVE_MOUNT, EPERM),
            Deny::call("fsopen", k::SYS_FSOPEN, EPERM),
            Deny::call("fsconfig", k::SYS_FSCONFIG, EPERM),
            Deny::call("fsmount", k::SYS_FSMOUNT, EPERM),
            Deny::call("fspick", k::SYS_FSPICK, EPERM),
            Deny::call("mount_setattr", k::SYS_MOUNT_SETATTR, EPERM),
            Deny::call("ptrace", k::SYS_PTRACE, EPERM),
            Deny::call("process_vm_readv", k::SYS_PROCESS_VM_READV, EPERM),
            Deny::call("process_vm_writev", k::SYS_PROCESS_VM_WRITEV, EPERM),
            Deny::call("keyctl", k::SYS_KEYCTL, EPERM),
            Deny::call("add_key", k::SYS_ADD_KEY, EPERM),
            Deny::call("request_key", k::SYS_REQUEST_KEY, EPERM),
            Deny::call("bpf", k::SYS_BPF, EPERM),
            Deny::call("perf_event_open", k::SYS_PERF_EVENT_OPEN, EPERM),
            Deny::call("kexec_load", k::SYS_KEXEC_LOAD, EPERM),
            Deny::call("init_module", k::SYS_INIT_MODULE, EPERM),
            Deny::call("finit_module", k::SYS_FINIT_MODULE, EPERM),
            Deny::call("delete_module", k::SYS_DELETE_MODULE, EPERM),
            Deny::call("open_by_handle_at", k::SYS_OPEN_BY_HANDLE_AT, EPERM),
            Deny::call("name_to_handle_at", k::SYS_NAME_TO_HANDLE_AT, EPERM),
        ];
        #[cfg(not(target_arch = "x86"))]
        v.push(Deny::call("kexec_file_load", k::SYS_KEXEC_FILE_LOAD, EPERM));
        #[cfg(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"))]
        v.push(Deny::call("umount", k::SYS_UMOUNT, EPERM));
        v.extend(Deny::io_uring());
        v
    }

    /// `io_uring_setup`, `io_uring_enter` and `io_uring_register` with
    /// `EPERM` (the answer of a kernel with `kernel.io_uring_disabled`): a
    /// ring makes a socket (`IORING_OP_SOCKET`) with no `socket(2)`, so a
    /// rule on `socket` is void while a ring is open.
    pub fn io_uring() -> Vec<Deny> {
        vec![
            Deny::call("io_uring_setup", k::SYS_IO_URING_SETUP, EPERM),
            Deny::call("io_uring_enter", k::SYS_IO_URING_ENTER, EPERM),
            Deny::call("io_uring_register", k::SYS_IO_URING_REGISTER, EPERM),
        ]
    }

    /// The rules of a closed UDP: `socket(2)` of `AF_INET` and `AF_INET6`
    /// with the type `SOCK_DGRAM` (a ping socket too) fails with `EACCES`.
    /// The type is masked, as `SOCK_NONBLOCK` and `SOCK_CLOEXEC` are bits
    /// of it.
    pub fn udp() -> Vec<Deny> {
        [("AF_INET", k::AF_INET), ("AF_INET6", k::AF_INET6)]
            .into_iter()
            .map(|(family, value)| {
                Deny::call(format!("socket({family}, SOCK_DGRAM)"), k::SYS_SOCKET, EACCES)
                    .when_equal(0, u32::MAX, value as u32)
                    .when_equal(1, k::SOCK_TYPE_MASK as u32, k::SOCK_DGRAM as u32)
            })
            .collect()
    }

    /// The rules of a TCP list: `socket(2)` of `AF_INET` and `AF_INET6`
    /// with `IPPROTO_MPTCP` fails with `ENOPROTOOPT`, the answer of a kernel
    /// with MPTCP off. Landlock checks TCP sockets only, so an MPTCP
    /// connect passes each port rule; a payload then falls back to TCP.
    pub fn mptcp() -> Vec<Deny> {
        [("AF_INET", k::AF_INET), ("AF_INET6", k::AF_INET6)]
            .into_iter()
            .map(|(family, value)| {
                Deny::call(format!("socket({family}, IPPROTO_MPTCP)"), k::SYS_SOCKET, ENOPROTOOPT)
                    .when_equal(0, u32::MAX, value as u32)
                    .when_equal(2, u32::MAX, k::IPPROTO_MPTCP as u32)
            })
            .collect()
    }

    /// The rules of a closed socket (a TCP list or a closed UDP) for the
    /// sockets that Landlock does not check. `socket(2)` and
    /// `socketpair(2)` of a family other than `AF_UNIX`, `AF_INET`,
    /// `AF_INET6` and `AF_NETLINK` fail with `EAFNOSUPPORT` (packet, VSOCK,
    /// SMC, RDS, RxRPC). `socket(2)` of `AF_INET` and `AF_INET6` of a type
    /// other than `SOCK_STREAM` and `SOCK_DGRAM` fails with
    /// `ESOCKTNOSUPPORT` (raw, SCTP over `SOCK_SEQPACKET`), and a
    /// `SOCK_STREAM` of a protocol other than 0 and `IPPROTO_TCP` fails
    /// with `EPROTONOSUPPORT` (SCTP, SMC). Each answer is the one of a
    /// kernel with no such family, type or protocol. Put [`Deny::mptcp`]
    /// first: the first rule of a number that holds gives the errno.
    pub fn families() -> Vec<Deny> {
        let kept = [k::AF_UNIX, k::AF_INET, k::AF_INET6, k::AF_NETLINK];
        let mut v: Vec<Deny> = [("socket", k::SYS_SOCKET), ("socketpair", k::SYS_SOCKETPAIR)]
            .into_iter()
            .map(|(name, nr)| {
                kept.iter().fold(Deny::call(format!("{name}(another family)"), nr, EAFNOSUPPORT), |d, &family| {
                    d.when_not_equal(0, u32::MAX, family as u32)
                })
            })
            .collect();
        let mask = k::SOCK_TYPE_MASK as u32;
        for (family, value) in [("AF_INET", k::AF_INET), ("AF_INET6", k::AF_INET6)] {
            v.push(
                Deny::call(format!("socket({family}, another type)"), k::SYS_SOCKET, ESOCKTNOSUPPORT)
                    .when_equal(0, u32::MAX, value as u32)
                    .when_not_equal(1, mask, k::SOCK_STREAM as u32)
                    .when_not_equal(1, mask, k::SOCK_DGRAM as u32),
            );
            v.push(
                Deny::call(format!("socket({family}, SOCK_STREAM, another protocol)"), k::SYS_SOCKET, EPROTONOSUPPORT)
                    .when_equal(0, u32::MAX, value as u32)
                    .when_equal(1, mask, k::SOCK_STREAM as u32)
                    .when_not_equal(2, u32::MAX, 0)
                    .when_not_equal(2, u32::MAX, k::IPPROTO_TCP as u32),
            );
        }
        v
    }
}

/// The BPF program of `deny`: the check of the arch, then one block for
/// each rule, then `ALLOW`. The first rule of a number that holds gives
/// the errno; a rule after a rule of its number with no condition never
/// runs, so the program leaves it out.
///
/// A refusal names the rule: an errno out of 1 to 4095, an argument index
/// over 5, a value outside its mask, or a program over `BPF_MAXINSNS`.
pub fn program(deny: &[Deny]) -> Result<Vec<SockFilter>, Failure> {
    use pod_hostprobe::sys::{bpf_jump as jump, bpf_stmt as stmt};
    let errno = |e: u32| k::SECCOMP_RET_ERRNO | e;
    let mut p = vec![
        stmt(k::BPF_LD_W_ABS, k::SECCOMP_DATA_ARCH),
        jump(k::BPF_JEQ_K, k::AUDIT_ARCH_NATIVE, 1, 0),
        stmt(k::BPF_RET_K, errno(u32::from(ENOSYS))),
        stmt(k::BPF_LD_W_ABS, k::SECCOMP_DATA_NR),
    ];
    if k::X32_SYSCALL_BIT != 0 {
        p.push(jump(k::BPF_JGE_K, k::X32_SYSCALL_BIT, 0, 1));
        p.push(stmt(k::BPF_RET_K, errno(u32::from(ENOSYS))));
    }
    // The numbers with a rule of no condition: a later rule never runs.
    let mut whole: Vec<u32> = Vec::new();
    for d in deny {
        if d.errno == 0 || u32::from(d.errno) > k::SECCOMP_MAX_ERRNO {
            return Err(failure(
                Class::Usage,
                format!("the deny rule of {} has the errno {}; a rule takes 1 to 4095", d.name, d.errno),
            ));
        }
        if whole.contains(&d.nr) {
            continue;
        }
        if d.when.is_empty() {
            whole.push(d.nr);
            p.push(jump(k::BPF_JEQ_K, d.nr, 0, 1));
            p.push(stmt(k::BPF_RET_K, errno(u32::from(d.errno))));
        } else {
            p.extend(conditional(d)?);
        }
    }
    p.push(stmt(k::BPF_RET_K, k::SECCOMP_RET_ALLOW));
    if p.len() > k::BPF_MAXINSNS {
        return Err(failure(
            Class::Usage,
            format!("the deny list makes {} BPF instructions; a program holds at most {}", p.len(), k::BPF_MAXINSNS),
        ));
    }
    Ok(p)
}

/// The block of a rule with conditions. Another call jumps past the block.
/// Each condition loads its argument and tests it; a condition that does
/// not hold jumps to the last instruction, which loads the number again,
/// so that the next rule of the number runs.
fn conditional(d: &Deny) -> Result<Vec<SockFilter>, Failure> {
    use pod_hostprobe::sys::{bpf_jump as jump, bpf_stmt as stmt};
    let usage = |text: String| failure(Class::Usage, format!("the deny rule of {}: {text}", d.name));
    let mut body = Vec::new();
    // The index in `body` of each jump to the last instruction, and true
    // where the jump is taken on a match (a "not equal").
    let mut misses: Vec<(usize, bool)> = Vec::new();
    for a in &d.when {
        let index = a.index();
        if index > 5 {
            return Err(usage(format!("it reads argument {index}; a call has 6 (0 to 5)")));
        }
        body.push(stmt(k::BPF_LD_W_ABS, k::seccomp_arg_low(u32::from(index))));
        match *a {
            Arg::AnyBit { mask, .. } => {
                misses.push((body.len(), false));
                body.push(jump(k::BPF_JSET_K, mask, 0, 0));
            }
            Arg::Equal { mask, value, .. } | Arg::NotEqual { mask, value, .. } => {
                let equal = matches!(a, Arg::Equal { .. });
                if value & !mask != 0 {
                    let never = if equal { "the rule never holds" } else { "the condition always holds" };
                    return Err(usage(format!("argument {index} masked with {mask:#x} is never {value:#x}; {never}")));
                }
                if mask != u32::MAX {
                    body.push(stmt(k::BPF_ALU_AND_K, mask));
                }
                misses.push((body.len(), !equal));
                body.push(jump(k::BPF_JEQ_K, value, 0, 0));
            }
        }
    }
    body.push(stmt(k::BPF_RET_K, k::SECCOMP_RET_ERRNO | u32::from(d.errno)));
    let reload = body.len();
    body.push(stmt(k::BPF_LD_W_ABS, k::SECCOMP_DATA_NR));
    let too_long = || usage(format!("its {} conditions make a jump over 255 instructions", d.when.len()));
    for (at, on_match) in misses {
        let to_reload = u8::try_from(reload - at - 1).map_err(|_| too_long())?;
        if on_match {
            body[at].jt = to_reload;
        } else {
            body[at].jf = to_reload;
        }
    }
    let past = u8::try_from(body.len()).map_err(|_| too_long())?;
    let mut block = Vec::with_capacity(body.len() + 1);
    block.push(jump(k::BPF_JEQ_K, d.nr, 0, past));
    block.extend(body);
    Ok(block)
}

#[cfg(test)]
mod tests;
