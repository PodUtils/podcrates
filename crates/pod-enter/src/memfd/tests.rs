use super::*;

#[test]
fn cloexec_is_the_unconditional_base() {
    assert_eq!(MFD_CLOEXEC, 0x0001, "linux/memfd.h MFD_CLOEXEC");
    assert_eq!(MFD_ALLOW_SEALING, 0x0002, "linux/memfd.h MFD_ALLOW_SEALING");
}

#[test]
fn scripts_route_past_fd_exec() {
    assert!(has_shebang(b"#!/bin/sh\necho hi\n"));
    assert!(!has_shebang(b"\x7fELF\x02\x01\x01\x00"));
    assert!(!has_shebang(b""));
    assert!(!has_shebang(b"# not a shebang\n"));
}

#[test]
fn executable_probe_names_what_can_exec() {
    pod_testkit::need_tool!("/bin/sh");
    assert!(is_executable(Path::new("/bin/sh")));
    assert!(!is_executable(Path::new("/nonexistent-pod-enter-probe-target")));
}

#[test]
fn memfd_round_trip_creates_a_usable_fd() {
    let name = sys::CBuf::new("pod-enter-probe").expect("literal has no NUL");
    let fd = unsafe { memfd_create(&name, false) }.expect("memfd_create runs here");
    assert!(fd >= 0);
    // The flag is on the descriptor, not only in the constant.
    let flags = sys::fcntl(fd, sys::F_GETFD, 0).expect("the created fd answers fcntl");
    assert_eq!(flags & sys::FD_CLOEXEC as i64, sys::FD_CLOEXEC as i64, "MFD_CLOEXEC is set on the created memfd");
    sys::close(fd).expect("the created fd closes");
}

/// An ELF file with no `PT_INTERP` is eligible; one with a `PT_INTERP` is
/// refused with the name of the loader, whatever its sections say.
#[test]
fn eligibility_is_the_program_headers_and_not_the_sections() {
    assert_eq!(eligible(&static_pie()), Ok(()));
    let Err(MemfdRefusal::HasInterp(interp)) = eligible(&dynamic_pie()) else {
        panic!("a PT_INTERP payload was not refused as one");
    };
    assert!(interp.contains("ld-linux"), "{interp}");
}

/// `#!` goes past the exec of a descriptor; other bytes get a named
/// refusal.
#[test]
fn scripts_route_past_and_garbage_is_named() {
    assert_eq!(eligible(b"#!/bin/sh\necho hi\n"), Err(MemfdRefusal::RoutePastScript));
    assert!(matches!(eligible(b""), Err(MemfdRefusal::NotElf(_))));
    assert!(matches!(eligible(b"not elf at all"), Err(MemfdRefusal::NotElf(_))));
}

/// The refusal names the first line, with no newline and no carriage
/// return, lossy past UTF-8, and cut where a hostile line is long.
#[test]
fn the_first_line_is_what_a_refusal_names() {
    assert_eq!(first_shebang_line(b"#!/bin/sh\necho hi\n"), "#!/bin/sh");
    assert_eq!(first_shebang_line(b"#!/bin/sh"), "#!/bin/sh");
    assert_eq!(first_shebang_line(b"#!/bin/sh\r\necho hi\n"), "#!/bin/sh");
    assert_eq!(first_shebang_line(b""), "");
    assert_eq!(first_shebang_line(b"\nrest"), "");
    // Lossy, not a failure: the refusal is a sentence about the bytes.
    assert!(first_shebang_line(b"#!\xff\xfe\n").starts_with("#!"));
    // Cut with its marker, on a character boundary.
    let long = format!("#!{}", "x".repeat(MAX_SHEBANG_LINE + 100));
    let got = first_shebang_line(long.as_bytes());
    assert!(got.ends_with("..."), "{got}");
    assert!(got.len() <= MAX_SHEBANG_LINE + 3, "{got}");
}

/// The refusal carries the resolved path as given and the first line
/// beside it, and the message names both.
#[test]
fn a_script_refusal_names_the_file_and_its_shebang() {
    let r = script_refusal("/store/rootfs/usr/bin/node", b"#!/usr/bin/env node\ncode\n");
    assert_eq!(r.path, "/store/rootfs/usr/bin/node");
    assert_eq!(r.shebang, "#!/usr/bin/env node");
    let text = r.message();
    assert!(text.contains("/store/rootfs/usr/bin/node"), "{text}");
    assert!(text.contains("#!/usr/bin/env node"), "{text}");
    assert!(text.contains("host's interpreter"), "{text}");
    // `eligible` keeps its form beside the helper.
    assert_eq!(eligible(b"#!/usr/bin/env node\n"), Err(MemfdRefusal::RoutePastScript));
}

/// Written bytes are sealed where the kernel accepts it, and run with no
/// seals where it does not.
#[test]
fn written_bytes_seal_where_accepted() {
    let name = sys::CBuf::new("pod-enter-seal").expect("literal has no NUL");
    let fd = unsafe { memfd_create(&name, true) }.expect("memfd_create runs here");
    let bytes = b"\x7fELF pod-enter seal probe";
    let n = unsafe { write_full(fd, bytes) }.expect("write_full writes all");
    assert_eq!(n, bytes.len() as i64);
    if let Err(e) = seal(fd) {
        // A kernel with no seals answers EINVAL. Each other errno is a
        // failure.
        assert_eq!(e, sys::EINVAL, "sealing failed with {e:?}");
    }
    sys::close(fd).expect("the created fd closes");
}

/// `stage` gives a live descriptor with close-on-exec set: the staged
/// memfd must not cross a later `execve`.
#[test]
fn staging_hands_back_a_live_cloexec_descriptor() {
    let fd = stage(&Names::DEFAULT, &static_pie()).expect("staging runs here");
    assert!(fd >= 0);
    let flags = sys::fcntl(fd, sys::F_GETFD, 0).expect("the staged fd answers fcntl");
    assert_eq!(flags & sys::FD_CLOEXEC as i64, sys::FD_CLOEXEC as i64, "MFD_CLOEXEC is set on the staged memfd");
    sys::close(fd).expect("the staged fd closes");
}

/// A 64-bit header with one `PT_LOAD` and no `PT_INTERP`: the smallest
/// bytes that `eligible` must accept.
fn static_pie() -> Vec<u8> {
    let mut b = header(1);
    b[64..68].copy_from_slice(&1u32.to_le_bytes());
    b
}

/// The same with one `PT_INTERP` that names a loader: `eligible` must
/// refuse it with the name of that loader.
fn dynamic_pie() -> Vec<u8> {
    let mut b = header(2);
    let interp = b"/lib64/ld-linux-x86-64.so.2\0";
    // First program header: `PT_LOAD`. Second: `PT_INTERP`, whose 64-bit
    // `p_offset` is at phdr+8 and points past both headers.
    b[64..68].copy_from_slice(&1u32.to_le_bytes());
    b[120..124].copy_from_slice(&3u32.to_le_bytes());
    b[128..136].copy_from_slice(&176u64.to_le_bytes());
    b.extend_from_slice(interp);
    b
}

/// A 64-bit little-endian header with `e_phoff = 64`, `e_phentsize = 56`
/// and space for `phnum` program headers.
fn header(phnum: u16) -> Vec<u8> {
    let mut b = vec![0u8; 64 + 56 * usize::from(phnum)];
    b[0..4].copy_from_slice(b"\x7fELF");
    b[4] = 2;
    b[5] = 1;
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&phnum.to_le_bytes());
    b
}
