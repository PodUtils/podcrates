//! Whether this machine can execute a foreign-architecture payload, and how.
//!
//! [`TODO/enter.md`](../../../TODO/enter.md) T-0506.
//!
//! ⭐ **This is the mechanism docker and podman use, read rather than assumed.**
//! `binfmt_misc` lets the kernel hand an ELF it cannot execute to a registered
//! interpreter, which for a foreign architecture is `qemu-user`. podvm reads
//! the registrations out of `/proc/sys/fs/binfmt_misc` and reports what it
//! found; it never registers one, because registering is a machine-wide change
//! and podvm is not the machine's owner.
//!
//! ⛔ **The `F` flag is the whole question for a chroot.** Without it the
//! kernel opens the interpreter **by path at exec time**, which is a path inside
//! the new root, so a chroot with no qemu in it fails. With it the interpreter
//! is opened when the registration is made and held, so it works inside any
//! chroot. Measured on 2026-09-09, `experiments/results/multiarch.txt` clause 5:
//! an `F` registration ran an aarch64 binary in a bare chroot containing
//! nothing but that binary.

use std::path::{Path, PathBuf};

// ⭐ The reader moved to `podvm-probe` and is re-exported here, so every
// existing caller keeps one name for it and there is exactly one parser.
// `podvm_probe::interp` needs the same registrations to answer the mirror
// question, "is an interpreter registered for podvm's OWN architecture".
pub use podvm_probe::binfmt::{machine_for, registrations, Registration, BINFMT_DIR};

/// What podvm can say about running `want` on this host.
#[derive(Debug, Clone)]
pub enum Support {
    /// The payload is this machine's own architecture.
    Native,
    /// A registered interpreter carrying `F`: it works inside the chroot.
    Interpreter(Registration),
    /// A registered interpreter WITHOUT `F`. ⚠ It works only if the interpreter
    /// is reachable by its own path inside the rootfs, which is the copy-in.
    InterpreterNeedsCopyIn(Registration),
    /// Nothing is registered for this architecture.
    None { why: String },
}

/// Whether this machine can run `arch`, and how.
///
/// ⛔ Never a guess. Where nothing is registered the answer carries **why**, so
/// the refusal names the remedy rather than the symptom.
pub fn support_for(arch: &str, host_arch: &str) -> Support {
    if arch == host_arch {
        return Support::Native;
    }
    let want = match machine_for(arch) {
        Some(m) => m,
        None => {
            return Support::None {
                why: format!(
                    "podvm has no ELF machine number for the architecture {arch:?}, so it \
                     cannot tell whether an interpreter is registered for it"
                ),
            }
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

/// The interpreter's path inside the rootfs, for the copy-in.
pub fn copy_in_target(root: &str, interpreter: &str) -> PathBuf {
    Path::new(root).join(interpreter.trim_start_matches('/'))
}

/// Copy a `binfmt_misc` interpreter into a rootfs.
///
/// ⛔ **This writes a file into somebody else's image and the caller MUST
/// disclose it.** T-0506: podvm's honesty rules have no exception for a helpful
/// edit, and the payload can see this file.
///
/// ⚠ Only ever called for a registration WITHOUT `F`. With `F` the kernel holds
/// the interpreter open already and copying it in would be a write with no
/// purpose.
pub fn copy_interpreter_in(root: &str, r: &Registration) -> std::io::Result<PathBuf> {
    let dest = copy_in_target(root, &r.interpreter);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // ⚠ Skipped where it is already there and the same size: re-copying a
    // 10 MB static qemu on every `run` is a cost with no benefit, and the
    // interpreter is a fixed artefact rather than something that drifts.
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
mod tests {
    use super::*;

    #[test]
    fn the_host_architecture_needs_no_interpreter() {
        assert!(matches!(support_for("amd64", "amd64"), Support::Native));
    }

    #[test]
    fn an_architecture_podvm_cannot_name_says_so_rather_than_guessing() {
        let s = support_for("nosucharch", "amd64");
        match s {
            Support::None { why } => assert!(why.contains("no ELF machine number"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_elf_machine_numbers_are_the_specifications() {
        // ⛔ Measured against real binaries rather than recalled: 0xb7 is what
        // `readelf -h` prints for an aarch64 ELF and 0x3e for x86-64, and
        // experiments/260-multiarch.sh registers a magic carrying 0xb7.
        assert_eq!(machine_for("arm64"), Some(0xb7));
        assert_eq!(machine_for("amd64"), Some(0x3e));
        assert_eq!(machine_for("riscv64"), Some(0xf3));
        assert_eq!(machine_for("nope"), None);
    }

    /// ⚠ Reads the real `/proc/sys/fs/binfmt_misc` where it is mounted, and
    /// asserts only what is true either way. A test that required a
    /// registration would fail on a machine that is simply configured
    /// differently, which is a test about the machine and not about podvm.
    #[test]
    fn a_missing_binfmt_directory_is_reported_and_not_a_panic() {
        let s = support_for("arm64", "amd64");
        match s {
            Support::None { why } => {
                assert!(!why.is_empty(), "a refusal with no reason");
            }
            Support::Interpreter(r) | Support::InterpreterNeedsCopyIn(r) => {
                assert!(!r.interpreter.is_empty());
                assert_eq!(r.machine, Some(0xb7));
            }
            Support::Native => panic!("arm64 is not amd64"),
        }
    }
}
