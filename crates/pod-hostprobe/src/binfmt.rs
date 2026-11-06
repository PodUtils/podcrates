//! What this machine's `binfmt_misc` registrations say, read rather than assumed.
//!
//! **This is the mechanism that docker and podman use.** `binfmt_misc` lets
//! the kernel hand an ELF that it cannot execute to a registered interpreter,
//! which for a foreign architecture is `qemu-user`. This module reads the
//! registrations out of `/proc/sys/fs/binfmt_misc` and reports what it found.
//! It never registers one: a registration is a change of the whole machine,
//! and the caller is not the owner of the machine.
//!
//! **It is in this crate because two different callers need it, and a
//! second copy drifts.** The entry of a payload asks "can this machine
//! execute a foreign image, and how" ([`super::interp`] does not);
//! [`super::interp`] asks the mirror-image question, "is an interpreter
//! registered for MY OWN architecture, so is this measurement the emulator's".
//!
//! **`/proc/sys/fs/binfmt_misc` is a real host file and `qemu-user` does not
//! emulate it**, which is what makes the second question answerable at all.
//! Measured: under `qemu-aarch64-static`, `/proc/cpuinfo` reports
//! an ARMv8 processor and `readlink /proc/self/exe` names the guest binary, so
//! both are the emulator's answers; `/proc/sys/fs/binfmt_misc` is the host's.

use std::path::Path;

/// Where the kernel exposes its registrations.
pub const BINFMT_DIR: &str = "/proc/sys/fs/binfmt_misc";

/// One registration, as this module reads it.
#[derive(Debug, Clone)]
pub struct Registration {
    pub name: String,
    pub interpreter: String,
    pub enabled: bool,
    /// The `F` flag: the interpreter is opened at registration time and held
    /// open, so it is reachable from inside a chroot that does not contain it.
    pub fix_binary: bool,
    /// The ELF `e_machine` the magic selects on, where the module could read one.
    pub machine: Option<u16>,
}

/// The ELF machine number for an OCI architecture name.
///
/// These are the ELF specification's `EM_*` values, and they are what a
/// `binfmt_misc` magic actually matches on. Mapping an OCI name to one is the
/// only way to answer "is there an interpreter for THIS image" rather than
/// "is there any interpreter at all".
pub fn machine_for(arch: &str) -> Option<u16> {
    Some(match arch {
        "amd64" => 0x3e,
        "386" => 0x03,
        "arm64" => 0xb7,
        "arm" => 0x28,
        "riscv64" => 0xf3,
        "ppc64le" | "ppc64" => 0x15,
        "s390x" => 0x16,
        "loong64" => 0x102,
        "mips64le" | "mips64" => 0x08,
        _ => return None,
    })
}

fn parse_one(path: &Path) -> Option<Registration> {
    let text = std::fs::read_to_string(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let mut interpreter = String::new();
    let mut enabled = false;
    let mut flags = String::new();
    let mut magic = String::new();
    let mut offset: usize = 0;
    for line in text.lines() {
        let line = line.trim();
        if line == "enabled" {
            enabled = true;
        } else if let Some(v) = line.strip_prefix("interpreter ") {
            interpreter = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("flags:") {
            flags = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("magic ") {
            magic = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("offset ") {
            offset = v.trim().parse().unwrap_or(0);
        }
    }
    // `e_machine` is a little-endian u16 at byte 18 of an ELF header, so it
    // is hex characters 36 and 37 of the magic when the registration starts at
    // offset 0. A magic shorter than that does not select on the architecture
    // at all, which is worth knowing rather than guessing at.
    let machine = {
        let start = 18usize.checked_sub(offset).map(|b| b * 2);
        match start {
            Some(s) if magic.len() >= s + 4 => {
                let lo = u8::from_str_radix(&magic[s..s + 2], 16).ok();
                let hi = u8::from_str_radix(&magic[s + 2..s + 4], 16).ok();
                match (lo, hi) {
                    (Some(l), Some(h)) => Some(u16::from(l) | (u16::from(h) << 8)),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    Some(Registration { name, interpreter, enabled, fix_binary: flags.contains('F'), machine })
}

/// Every enabled registration this machine carries.
pub fn registrations() -> Vec<Registration> {
    let dir = match std::fs::read_dir(BINFMT_DIR) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for e in dir.flatten() {
        let name = e.file_name();
        // `register` and `status` are the control files, not registrations.
        if name == "register" || name == "status" {
            continue;
        }
        if let Some(r) = parse_one(&e.path())
            && r.enabled
        {
            out.push(r);
        }
    }
    out
}

/// The ELF `e_machine` of the binary you are reading this in.
///
/// **The OWN machine number of this binary**, which is a different question
/// from the image's and is the one [`super::interp`] asks. It comes from
/// `target_arch` here, in one place. A caller that maps the OCI name of the
/// host platform with [`machine_for`] must check that the two agree: a value
/// in two places with no check is the value that drifts.
pub const SELF_MACHINE: u16 = self_machine();

const fn self_machine() -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        0x3e
    }
    #[cfg(target_arch = "aarch64")]
    {
        0xb7
    }
    #[cfg(target_arch = "x86")]
    {
        0x03
    }
    #[cfg(target_arch = "arm")]
    {
        0x28
    }
    #[cfg(target_arch = "riscv64")]
    {
        0xf3
    }
    #[cfg(target_arch = "loongarch64")]
    {
        0x102
    }
    #[cfg(target_arch = "powerpc64")]
    {
        0x15
    }
    #[cfg(target_arch = "s390x")]
    {
        0x16
    }
    // No fallback. On an architecture with no machine number here, the
    // emulator check would silently answer "native".
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "loongarch64",
        target_arch = "powerpc64",
        target_arch = "s390x",
    )))]
    {
        compile_error!("pod-hostprobe has no ELF machine number for this target_arch; add it to src/binfmt.rs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_elf_machine_numbers_are_the_specifications() {
        // Measured against real binaries, not recalled: 0xb7 is what
        // `readelf -h` prints for an aarch64 ELF and 0x3e for x86-64.
        assert_eq!(machine_for("arm64"), Some(0xb7));
        assert_eq!(machine_for("amd64"), Some(0x3e));
        assert_eq!(machine_for("riscv64"), Some(0xf3));
        assert_eq!(machine_for("nope"), None);
    }

    /// Reads the real `/proc/sys/fs/binfmt_misc` where it is mounted and
    /// asserts only what is true either way. A test that required a
    /// registration would be a test about this machine and not about the crate.
    #[test]
    fn reading_the_registrations_is_never_a_panic() {
        for r in registrations() {
            assert!(!r.name.is_empty());
            assert!(r.enabled, "registrations() returns only enabled ones");
        }
    }
}
