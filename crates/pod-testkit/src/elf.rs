//! A static ELF made in the test, for a test of an exec that needs a
//! program with no loader and no tool of the host.

/// A static ELF executable of the architecture of the build that calls
/// `exit_group(0)`: one `PT_LOAD` segment, no `PT_INTERP`, no section.
/// `None` on an architecture other than x86_64 and aarch64.
pub fn static_exit0() -> Option<Vec<u8>> {
    let (machine, code): (u16, &[u8]) = if cfg!(target_arch = "x86_64") {
        // xor edi, edi; mov eax, 231; syscall
        (0x3e, &[0x31, 0xff, 0xb8, 0xe7, 0, 0, 0, 0x0f, 0x05])
    } else if cfg!(target_arch = "aarch64") {
        // mov x0, #0; mov x8, #94; svc #0
        (0xb7, &[0x00, 0x00, 0x80, 0xd2, 0xc8, 0x0b, 0x80, 0xd2, 0x01, 0x00, 0x00, 0xd4])
    } else {
        return None;
    };
    let base: u64 = 0x40_0000;
    let len = (HEADERS + code.len()) as u64;
    let mut f = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    f.extend(2u16.to_le_bytes()); // ET_EXEC
    f.extend(machine.to_le_bytes());
    f.extend(1u32.to_le_bytes());
    f.extend((base + HEADERS as u64).to_le_bytes()); // the entry: the code after the headers
    f.extend(64u64.to_le_bytes()); // e_phoff
    f.extend(0u64.to_le_bytes()); // e_shoff
    f.extend(0u32.to_le_bytes());
    for half in [64u16, 56, 1, 64, 0, 0] {
        f.extend(half.to_le_bytes());
    }
    f.extend(1u32.to_le_bytes()); // PT_LOAD
    f.extend(5u32.to_le_bytes()); // read and execute
    for word in [0, base, base, len, len, 0x1000] {
        f.extend(word.to_le_bytes());
    }
    f.extend_from_slice(code);
    Some(f)
}

/// The ELF header and the one program header.
const HEADERS: usize = 64 + 56;

/// Whether `bytes` are a 64-bit ELF file with a `PT_INTERP` segment: a
/// dynamic program.
pub fn is_dynamic(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || bytes[..4] != *b"\x7fELF" || bytes[4] != 2 {
        return false;
    }
    let u = |at: usize, n: usize| bytes[at..at + n].iter().rev().fold(0usize, |a, x| (a << 8) | usize::from(*x));
    let (phoff, phentsize, phnum) = (u(32, 8), u(54, 2), u(56, 2));
    (0..phnum).any(|i| {
        let at = phoff.saturating_add(i.saturating_mul(phentsize));
        bytes.get(at..at.saturating_add(4)) == Some(&3u32.to_le_bytes()[..])
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_static_elf_has_no_interp_and_runs() {
        let Some(elf) = super::static_exit0() else { return };
        assert!(!super::is_dynamic(&elf));
        assert_eq!(elf.len(), super::HEADERS + if cfg!(target_arch = "x86_64") { 9 } else { 12 });
        let p = std::env::temp_dir().join(format!("pod-testkit-elf-{}", std::process::id()));
        std::fs::write(&p, &elf).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let status = std::process::Command::new(&p).status().unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(status.success(), "{status}");
    }
}
