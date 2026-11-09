//! Can this machine execute a payload of a foreign architecture, and
//! how?
//!
//! The mechanism of docker and podman, read and not assumed:
//! `binfmt_misc` lets the kernel give an ELF file that it cannot execute
//! to a registered interpreter, which for a foreign architecture is
//! `qemu-user`. This module reads the registrations from
//! `/proc/sys/fs/binfmt_misc` and reports what it found. It never
//! registers one: a registration changes the whole machine.
//!
//! The `F` flag decides the case of a chroot. With no `F` the kernel opens
//! the interpreter by path at the time of the exec, which is a path inside
//! the new root, so a chroot with no qemu in it fails. With `F` the kernel
//! opens the interpreter when the registration is made and holds it, so it
//! works inside each chroot: an `F` registration ran an aarch64 binary in
//! a bare chroot that held only that binary.

use std::path::{Path, PathBuf};

// The reader is in `pod-hostprobe`: its `interp` needs the same
// registrations for the other question, "is an interpreter registered for
// the architecture of this process". One parser.
pub use pod_hostprobe::binfmt::{BINFMT_DIR, Registration, machine_for, registrations};

/// What the entry can say about a run of `want` on this host.
#[derive(Debug, Clone)]
pub enum Support {
    /// The payload is this machine's own architecture.
    Native,
    /// A registered interpreter carrying `F`: it works inside the chroot.
    Interpreter(Registration),
    /// A registered interpreter with no `F`. It works only where the
    /// interpreter is at its own path inside the root file system: the copy.
    InterpreterNeedsCopyIn(Registration),
    /// Nothing is registered for this architecture.
    None { why: String },
}

/// Whether this machine can run `arch`, and how.
///
/// Never a guess. Where nothing is registered, the answer carries the
/// reason, so the refusal names the remedy and not the symptom.
pub fn support_for(arch: &str, host_arch: &str) -> Support {
    if arch == host_arch {
        return Support::Native;
    }
    let want = match machine_for(arch) {
        Some(m) => m,
        None => {
            return Support::None {
                why: format!(
                    "no ELF machine number is known for the architecture {arch:?}, so the entry \
                     cannot tell whether an interpreter is registered for it"
                ),
            };
        }
    };
    if !Path::new(BINFMT_DIR).is_dir() {
        return Support::None {
            why: format!(
                "{BINFMT_DIR} is not mounted, so this kernel has no interpreter \
                 registrations at all. `mount -t binfmt_misc none {BINFMT_DIR}` \
                 exposes them where the kernel was built with CONFIG_BINFMT_MISC"
            ),
        };
    }
    let all = registrations();
    for r in &all {
        if r.machine == Some(want) {
            return if r.fix_binary {
                Support::Interpreter(r.clone())
            } else {
                Support::InterpreterNeedsCopyIn(r.clone())
            };
        }
    }
    Support::None {
        why: format!(
            "no enabled binfmt_misc registration selects ELF machine 0x{want:x} ({arch}). \
             {} registration(s) are enabled and none of them match. Installing \
             qemu-user-static and registering it is what docker and podman rely on too",
            all.len()
        ),
    }
}

/// The path of the interpreter inside the root file system, for the copy.
pub fn copy_in_target(root: &str, interpreter: &str) -> PathBuf {
    Path::new(root).join(interpreter.trim_start_matches('/'))
}

/// Copy a `binfmt_misc` interpreter into a root file system.
///
/// This writes a file into the image of another party, and the caller
/// must tell the user so: the payload can see this file.
///
/// Call it only for a registration with no `F`. With `F` the kernel holds
/// the interpreter open already, and a copy would be a write with no
/// purpose.
pub fn copy_interpreter_in(root: &str, r: &Registration) -> std::io::Result<PathBuf> {
    let dest = copy_in_target(root, &r.interpreter);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // No copy where the file is there with the same size: a copy of a
    // static qemu of 10 MB on each run costs time for no benefit, and the
    // interpreter is a fixed file.
    if let (Ok(a), Ok(b)) = (std::fs::metadata(&r.interpreter), std::fs::metadata(&dest))
        && a.len() == b.len()
    {
        return Ok(dest);
    }
    std::fs::copy(&r.interpreter, &dest)?;
    // The interpreter has to be executable inside the rootfs.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests;
