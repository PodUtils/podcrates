//! The seccomp deny list and its BPF program.
//!
//! The program checks the audit arch first: a call of another ABI of the
//! same kernel (i386 or x32 on x86_64) has other numbers and would pass
//! the list, so it gets `ENOSYS`. Then each call of the list returns its
//! errno, and each other call runs.

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
const ENOSYS: u16 = 38;

/// One call that fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deny {
    /// The name of the call, for a reader.
    pub name: String,
    /// The number of the call on the build target.
    pub nr: u32,
    /// The errno that the call gets (1 to 4095).
    pub errno: u16,
    /// `Some((index, mask))`: the call fails only when the low 32 bits of
    /// its argument `index` (0 to 5) have a bit of `mask`; else it runs.
    pub flags: Option<(u8, u32)>,
}

impl Deny {
    /// The call `nr` (a `SYS_*` number of `pod_hostprobe::sys`) fails with
    /// `errno`.
    pub fn call(name: impl Into<String>, nr: i64, errno: u16) -> Deny {
        Deny { name: name.into(), nr: nr as u32, errno, flags: None }
    }

    /// The call fails only when its argument `index` has a bit of `mask`.
    pub fn when_flags(mut self, index: u8, mask: u32) -> Deny {
        self.flags = Some((index, mask));
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
    /// `open_by_handle_at` and `name_to_handle_at`. Each other one gets
    /// `EPERM`.
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
        v
    }
}

/// The BPF program of `deny`: the check of the arch, then one block for
/// each call (the first rule of a number wins), then `ALLOW`.
///
/// A refusal names the rule: an errno out of 1 to 4095, an argument index
/// over 5, or a program over `BPF_MAXINSNS`.
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
    let mut seen: Vec<u32> = Vec::new();
    for d in deny {
        if d.errno == 0 || u32::from(d.errno) > k::SECCOMP_MAX_ERRNO {
            return Err(failure(
                Class::Usage,
                format!("the deny rule of {} has the errno {}; a rule takes 1 to 4095", d.name, d.errno),
            ));
        }
        if seen.contains(&d.nr) {
            continue;
        }
        seen.push(d.nr);
        match d.flags {
            None => {
                p.push(jump(k::BPF_JEQ_K, d.nr, 0, 1));
                p.push(stmt(k::BPF_RET_K, errno(u32::from(d.errno))));
            }
            Some((index, mask)) => {
                if index > 5 {
                    return Err(failure(
                        Class::Usage,
                        format!("the deny rule of {} reads argument {index}; a call has 6 (0 to 5)", d.name),
                    ));
                }
                // Not this call: past the four below. A flag of the mask:
                // the errno; else the call runs, as each number has one
                // rule.
                p.push(jump(k::BPF_JEQ_K, d.nr, 0, 4));
                p.push(stmt(k::BPF_LD_W_ABS, k::seccomp_arg_low(u32::from(index))));
                p.push(jump(k::BPF_JSET_K, mask, 0, 1));
                p.push(stmt(k::BPF_RET_K, errno(u32::from(d.errno))));
                p.push(stmt(k::BPF_RET_K, k::SECCOMP_RET_ALLOW));
            }
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

#[cfg(test)]
mod tests;
