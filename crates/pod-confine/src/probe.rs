//! The probe of the host: the Landlock ABI, and an [`apply`] of a minimal
//! ruleset and filter in a disposable child, so that a nested ruleset under
//! the policy of the host (and a seccomp filter) is proven, and the caller
//! is not restricted.

use pod_core::{Class, Failure};
use pod_hostprobe::sys::{self as k, Errno};

use crate::{Compiled, Refusal, apply, errno_text, failure};

/// The ABI of the kernel where a confinement can load here, else `None`
/// ([`check`] says why).
pub fn probe() -> Option<u32> {
    check().ok()
}

/// The Landlock ABI of the kernel, where a ruleset and a seccomp filter
/// load in a child of this process. A refusal names the call and the
/// errno: `EOPNOTSUPP` or `ENOSYS` is a kernel with no Landlock; a
/// refusal of `landlock_restrict_self` (`E2BIG`: too many nested
/// domains, or `EPERM`) is the policy of the host.
pub fn check() -> Result<u32, Failure> {
    let abi = k::landlock_abi().map_err(|e| {
        failure(
            Class::Unsupported,
            format!(
                "Landlock is absent (landlock_create_ruleset(LANDLOCK_CREATE_RULESET_VERSION): {}): the payload \
                 does not run unconfined",
                errno_text(e)
            ),
        )
    })? as u32;
    // A ruleset of one handled right and no rule, and a filter that allows
    // each call: the steps of `apply`, with no effect on the child's work.
    let minimal = Compiled {
        abi,
        handled_fs: u64::from(k::LANDLOCK_ACCESS_FS_READ_FILE),
        handled_net: 0,
        rules: Vec::new(),
        ports: Vec::new(),
        filter: vec![k::bpf_stmt(k::BPF_RET_K, k::SECCOMP_RET_ALLOW)],
    };
    match in_child(&minimal)? {
        None => Ok(abi),
        Some(r) => Err(failure(
            crate::errno_class(r.errno),
            format!("Landlock ABI {abi} is here, and a confined child failed at {}", r.describe(&minimal)),
        )),
    }
}

/// Run [`apply`] of `c` in a child that then exits: `None` when each step
/// passed, else the refusal.
fn in_child(c: &Compiled) -> Result<Option<Refusal>, Failure> {
    let mut pipe = [0i32; 2];
    k::pipe2(&mut pipe, k::O_CLOEXEC).map_err(|e| errno_failure("pipe2 for the probe child", e))?;
    let (read_end, write_end) = (i64::from(pipe[0]), i64::from(pipe[1]));
    // SAFETY: the child calls only `apply` (system calls on data that
    // exists, no allocation), `write` and `exit_group`.
    let pid = match unsafe { k::clone_fork(k::SIGCHLD) } {
        Ok(0) => {
            let _ = k::close(read_end);
            if let Err(r) = apply(c) {
                let _ = k::write(write_end, &r.to_bytes());
            }
            k::exit_group(0)
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = (k::close(read_end), k::close(write_end));
            return Err(errno_failure("clone(2) for the probe child", e));
        }
    };
    let _ = k::close(write_end);
    let mut buf = [0u8; 5];
    let got = loop {
        match k::read(read_end, &mut buf) {
            Err(e) if e == k::EINTR => continue,
            other => break other,
        }
    };
    let _ = k::close(read_end);
    let mut status = 0;
    let waited = loop {
        match k::wait4(pid, &mut status) {
            Err(e) if e == k::EINTR => continue,
            other => break other,
        }
    };
    if let Err(e) = waited {
        return Err(errno_failure("wait4 of the probe child", e));
    }
    match got {
        Ok(5) => Refusal::from_bytes(buf)
            .map(Some)
            .ok_or_else(|| failure(Class::Unavailable, format!("the probe child wrote no refusal: {buf:?}"))),
        // No refusal: only an exit with 0 is a pass, not a kill.
        Ok(0) if status == 0 => Ok(None),
        Ok(0) => Err(failure(Class::Unavailable, format!("the probe child ended with the wait status {status}"))),
        Ok(n) => Err(failure(Class::Unavailable, format!("the probe child wrote {n} bytes of a refusal of 5"))),
        Err(e) => Err(errno_failure("the read of the probe child", e)),
    }
}

fn errno_failure(what: &str, e: Errno) -> Failure {
    failure(crate::errno_class(e), format!("{what}: {}", errno_text(e)))
}
