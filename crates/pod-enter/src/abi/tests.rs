use super::*;
use std::os::unix::process::CommandExt;

/// `resolve_in` never lists a directory that it gets.
///
/// The test stages a root file system whose directories deny a listing
/// and allow a traversal (mode 111), then runs the worker half below
/// again as `nobody` (uid and gid 65534). The worker first proves that the
/// fixture holds (a listing fails with `EACCES`), then resolves a payload
/// by name, which must pass: a resolver that lists would fail there.
///
/// It needs root to start a child as `nobody`. `EPERM` on the start means
/// that this host cannot drop the privilege, and the test skips. A worker
/// that is still privileged fails, so that it cannot pass with no proof.
#[test]
fn resolve_past_a_denied_listing() {
    pod_testkit::need_root!();
    let d = std::env::temp_dir().join(format!("pod-enter-nolist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::write(d.join("bin/prog"), b"\x7fELF").unwrap();
    // 111: traverse by name, never list. Both levels: the walk must
    // cross two denied listings to reach the payload.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o111)).unwrap();
    std::fs::set_permissions(d.join("bin"), std::fs::Permissions::from_mode(0o111)).unwrap();
    let me = std::env::current_exe().unwrap();
    let mut cmd = std::process::Command::new(me);
    cmd.arg("--exact")
        .arg("abi::tests::worker_resolves_past_a_denied_listing")
        .env("POD_ENTER_TEST_NOBODY_ROOT", &d)
        .env("POD_ENTER_TEST_NOBODY_GUEST", "bin/prog")
        .current_dir(std::env::temp_dir())
        .uid(65534)
        .gid(65534);
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.raw_os_error() == Some(libc_eperm()) => {
            pod_testkit::host::skip_line(&format!("this host cannot run a child as nobody ({e})"));
            let _ = std::fs::remove_dir_all(&d);
            return;
        }
        // A temporary directory under a directory that nobody cannot
        // cross (TMPDIR under /root): the child cannot enter it.
        Err(e) if e.raw_os_error() == Some(libc_eacces()) => {
            pod_testkit::host::skip_line(&format!("nobody cannot enter the temporary directory ({e})"));
            let _ = std::fs::remove_dir_all(&d);
            return;
        }
        Err(e) => panic!("spawning the worker failed: {e}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "the nobody worker did not prove by-name resolution:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `EPERM` with no `libc` crate: the number of the kernel, the same on
/// each Linux architecture.
fn libc_eperm() -> i32 {
    1
}

/// `EACCES`, the same on each Linux architecture.
fn libc_eacces() -> i32 {
    13
}

/// The worker half of [`resolve_past_a_denied_listing`].
///
/// It runs only where that test started it (under a plain `cargo test`
/// the variables are absent, and it returns at once). A listing that
/// passes here means that the process is still privileged, which proves
/// nothing: it panics and does not pass.
#[test]
fn worker_resolves_past_a_denied_listing() {
    let Ok(root) = std::env::var("POD_ENTER_TEST_NOBODY_ROOT") else {
        return;
    };
    let guest = std::env::var("POD_ENTER_TEST_NOBODY_GUEST").expect("the orchestrator sets both halves");
    match std::fs::read_dir(&root) {
        Ok(_) => panic!(
            "the worker can still list {root}: it is privileged, so a \
                 successful resolve would prove nothing"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => panic!("the fixture is broken: listing failed with {e} instead of EACCES"),
    }
    let got = resolve_in(std::path::Path::new(&root), &guest)
        .unwrap_or_else(|k| panic!("by-name resolve past a denied listing failed: {k:?}"));
    assert!(got.ends_with("prog"), "{got:?}");
}

/// The SONAME tells the C libraries apart (fact 1 of the module).
#[test]
fn the_soname_discriminator_is_the_one_that_was_measured() {
    assert_eq!(Flavour::of_libc_name("libc.so.6"), Flavour::Glibc);
    assert_eq!(Flavour::of_libc_name("libc.so"), Flavour::Musl);
    assert_eq!(Flavour::of_libc_name("/lib/ld-musl-x86_64.so.1"), Flavour::Musl);
    assert_eq!(Flavour::of_libc_name("/lib/libc.musl-aarch64.so.1"), Flavour::Musl);
    assert_eq!(Flavour::of_libc_name("/lib64/ld-linux-x86-64.so.2"), Flavour::Glibc);
    // A third state for each other name, never a guess.
    assert_eq!(Flavour::of_libc_name("libz.so.1"), Flavour::Unknown);
    assert_eq!(Flavour::of_needed(&["libgcc_s.so.1".into(), "libc.so.6".into()]), Flavour::Glibc);
}

/// The reader reads this test binary, the only ELF file that each host of
/// the test has. It can be static, with no `PT_INTERP` and no
/// `DT_NEEDED`: the test checks that the header and the section table
/// parse, and that the machine is the machine of this architecture.
#[test]
fn this_binarys_own_header_parses() {
    let me = std::env::current_exe().unwrap();
    let got = Elf::read(me.to_str().unwrap()).unwrap();
    assert_eq!(got.machine, pod_hostprobe::binfmt::SELF_MACHINE);
    // A caller finds Go payloads by the section names. With no readable
    // section table the caller has nothing to match.
    assert!(!got.sections.is_empty(), "this binary parsed with no section names");
}

/// The trap of `.symtab` (fact 4 of the module), as a check. A shipped
/// `libc.so.6` has 0 defined symbols in `.symtab` and about 3136 in
/// `.dynsym`, so a reader of `.symtab` refuses each object. This runs only
/// where the host has a libc; elsewhere it skips, because "could not run"
/// is a third state.
#[test]
fn a_shipped_libc_defines_its_symbols_in_dynsym() {
    let Some(p) =
        ["/lib/x86_64-linux-gnu/libc.so.6", "/lib64/libc.so.6", "/usr/lib/libc.so.6", "/lib/ld-musl-x86_64.so.1"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
    else {
        pod_testkit::host::skip_line("this host ships no libc at a conventional path");
        return;
    };
    let libc = Elf::read(p).unwrap();
    assert!(
        libc.defined.len() > 100,
        "{p} defined only {} symbols; .symtab was read instead of .dynsym",
        libc.defined.len()
    );
    assert!(libc.defines(&Versioned { name: "open".into(), version: None, weak: false, file: None }));
    assert_ne!(libc.flavour(), Flavour::Unknown, "{p}");
}

/// The version predicate (fact 3 of the module): an import at a version
/// that the libc does not declare is refused, and the refusal names the
/// version that the loader would name.
#[test]
fn an_import_at_a_version_the_libc_does_not_declare_is_refused() {
    let libc = fake(
        "/lib/libc.so.6",
        Some("libc.so.6"),
        &[],
        &[("open", None), ("snprintf", Some("GLIBC_2.4"))],
        &["GLIBC_2.4", "GLIBC_2.31"],
    );
    let obj = fake_import("/i.so", &["libc.so.6"], &[("open", None), ("snprintf", Some("GLIBC_2.34"))]);
    let v = admits(&obj, &libc);
    let Verdict::Refused(why) = v else {
        panic!("admitted an import at GLIBC_2.34 against a libc declaring 2.31");
    };
    assert!(why.contains("GLIBC_2.34"), "{why}");
    assert!(why.contains("GLIBC_2.31"), "{why}");
}

/// A musl root file system that gets a glibc object says so in those
/// words, not through a relocation error.
#[test]
fn a_glibc_object_against_a_musl_rootfs_is_refused_in_those_words() {
    let libc = fake("/lib/ld-musl-x86_64.so.1", None, &[], &[("open", None)], &[]);
    let obj = fake_import("/i.so", &["libc.so.6"], &[("open", None)]);
    let Verdict::Refused(why) = admits(&obj, &libc) else {
        panic!("a glibc object was admitted into a musl rootfs");
    };
    assert!(why.contains("musl"), "{why}");
    assert!(why.contains("glibc"), "{why}");
}

/// The matching pair is admitted: else the check would be a refusal, not a
/// gate.
#[test]
fn a_matching_object_and_libc_are_admitted() {
    let libc = fake(
        "/lib/libc.so.6",
        Some("libc.so.6"),
        &[],
        &[("open", None), ("snprintf", Some("GLIBC_2.4"))],
        &["GLIBC_2.4"],
    );
    let obj = fake_import("/i.so", &["libc.so.6"], &[("open", None), ("snprintf", Some("GLIBC_2.4"))]);
    assert_eq!(admits(&obj, &libc), Verdict::Admitted);
}

/// A symbol that the libc does not define is refused, and the message names
/// the symbol.
#[test]
fn an_undefined_symbol_is_refused_by_name() {
    let libc = fake("/lib/libc.so.6", Some("libc.so.6"), &[], &[("open", None)], &[]);
    let obj = fake_import("/i.so", &["libc.so.6"], &[("__snprintf_chk", None)]);
    let Verdict::Refused(why) = admits(&obj, &libc) else {
        panic!("admitted an object importing a symbol the libc does not define");
    };
    assert!(why.contains("__snprintf_chk"), "{why}");
    assert!(why.contains(".dynsym"), "{why}");
}

/// The libc comes from the interpreter of the payload, not from a table of
/// distributions.
#[test]
fn the_libc_is_found_beside_the_interpreter() {
    let d = std::env::temp_dir().join(format!("pod-enter-abi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lib/x86_64-linux-gnu")).unwrap();
    std::fs::write(d.join("lib/x86_64-linux-gnu/libc.so.6"), b"x").unwrap();
    std::fs::write(d.join("lib/ld-musl-x86_64.so.1"), tiny_elf(pod_hostprobe::binfmt::SELF_MACHINE)).unwrap();
    let root = d.to_string_lossy().to_string();
    let m = pod_hostprobe::binfmt::SELF_MACHINE;
    assert_eq!(
        libc_beside(&root, "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", m),
        Some(format!("{root}/lib/x86_64-linux-gnu/libc.so.6"))
    );
    // On musl the interpreter is the C library: the path as it is parses,
    // so it comes back with no search.
    assert_eq!(libc_beside(&root, "/lib/ld-musl-x86_64.so.1", m), Some(format!("{root}/lib/ld-musl-x86_64.so.1")));
    // An unknown interpreter gets no answer, not a guess.
    assert_eq!(libc_beside(&root, "/opt/weird/loader", m), None);
    let _ = std::fs::remove_dir_all(&d);
}

pub(super) fn tiny_elf(machine: u16) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0..4].copy_from_slice(b"\x7fELF");
    b[4] = 2;
    b[5] = 1;
    b[18..20].copy_from_slice(&machine.to_le_bytes());
    b
}

/// The multiarch layout, where the directory of the loader holds no libc:
/// Debian keeps the loader in `/lib64` and the library in
/// `/lib/<triplet>/`. The search finds it by name and machine, and a file
/// of another machine does not win.
#[test]
fn the_libc_is_found_across_a_multiarch_split() {
    let d = std::env::temp_dir().join(format!("pod-enter-abi64-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    const ME: u16 = 62;
    const OTHER: u16 = 183;
    std::fs::create_dir_all(d.join("lib64")).unwrap();
    std::fs::create_dir_all(d.join("lib/aarch64-linux-gnu")).unwrap();
    std::fs::create_dir_all(d.join("lib/x86_64-linux-gnu")).unwrap();
    std::fs::write(d.join("lib64/ld-linux-x86-64.so.2"), b"x").unwrap();
    std::fs::write(d.join("lib/aarch64-linux-gnu/libc.so.6"), tiny_elf(OTHER)).unwrap();
    std::fs::write(d.join("lib/x86_64-linux-gnu/libc.so.6"), tiny_elf(ME)).unwrap();
    let root = d.to_string_lossy().to_string();
    // The directory of the loader has no libc, so the direct rule misses
    // and the search must find the one of the machine.
    assert_eq!(
        libc_beside(&root, "/lib64/ld-linux-x86-64.so.2", ME),
        Some(format!("{root}/lib/x86_64-linux-gnu/libc.so.6"))
    );
    // A machine with no library here gets no answer, not the library of
    // the other architecture.
    assert_eq!(libc_beside(&root, "/lib64/ld-linux-x86-64.so.2", 40), None);
    let _ = std::fs::remove_dir_all(&d);
}

/// The musl loader in another place: an image keeps it out of `/lib`, and
/// the path as it is names nothing. The search finds it by its base name,
/// which names the architecture.
#[test]
fn a_musl_loader_outside_lib_is_found_by_name() {
    let d = std::env::temp_dir().join(format!("pod-enter-abi-musl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("usr/lib")).unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("usr/lib/ld-musl-x86_64.so.1"), b"fake-libc").unwrap();
    let root = d.to_string_lossy().to_string();
    let m = pod_hostprobe::binfmt::SELF_MACHINE;
    assert_eq!(libc_beside(&root, "/lib/ld-musl-x86_64.so.1", m), Some(format!("{root}/usr/lib/ld-musl-x86_64.so.1")));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_newest_declared_version_is_reported_numerically() {
    // Not over the whole string as text: `GLIBC_2.9` sorts after
    // `GLIBC_2.34` as text and is older.
    let v: Vec<String> = ["GLIBC_2.4", "GLIBC_2.34", "GLIBC_2.9"].iter().map(|s| s.to_string()).collect();
    assert_eq!(newest(&v), "GLIBC_2.34");
}

#[test]
fn a_file_that_is_not_elf_is_a_named_refusal_and_not_a_panic() {
    let e = Elf::parse("/x", b"#!/bin/sh\necho hi\n").unwrap_err();
    assert!(format!("{e}").contains("not an ELF file"), "{e}");
    // A short one, the form of a hostile image.
    let e = Elf::parse("/x", b"\x7fELF\x02\x01").unwrap_err();
    assert!(format!("{e}").contains("not an ELF file"), "{e}");
}

/// An offset or a count of a hostile file that is past the file, or past
/// a `usize`, gives no answer and no panic, also on a 32-bit target.
#[test]
fn a_hostile_offset_is_a_refusal_and_not_a_panic() {
    let mut b = tiny_elf(pod_hostprobe::binfmt::SELF_MACHINE);
    // One program header at the end of the address space.
    b[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    // "Unknown", never "static": the table is not in the bytes.
    let e = interp_of("/x", &b).unwrap_err();
    assert!(format!("{e}").contains("past the 64 bytes read"), "{e}");
    // A section table at the end of the address space.
    b[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    b[58..60].copy_from_slice(&64u16.to_le_bytes());
    let e = Elf::parse("/x", &b).unwrap_err();
    assert!(format!("{e}").contains("no section table"), "{e}");
    // Section 0 inside the file, whose size field gives a huge count: no
    // allocation of that size.
    let mut b = tiny_elf(pod_hostprobe::binfmt::SELF_MACHINE);
    b.resize(128, 0);
    b[40..48].copy_from_slice(&64u64.to_le_bytes());
    b[58..60].copy_from_slice(&64u16.to_le_bytes());
    b[64 + 32..64 + 40].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
    let e = Elf::parse("/x", &b).unwrap_err();
    assert!(format!("{e}").contains("no section table"), "{e}");
}

/// Head safety: where the program headers or the `PT_INTERP` path lie
/// past the bytes in hand, the answer is "unknown" (an error), never
/// "static". The whole file gives the true answer.
#[test]
fn a_header_past_the_head_is_unknown_and_never_static() {
    let head = usize::try_from(HEAD_BYTES).unwrap();
    // The program header table past the head.
    let b = dyn_elf(HEAD_BYTES + 4096, HEAD_BYTES + 8192);
    let e = interp_of("/x", &b[..head]).unwrap_err();
    assert!(format!("{e}").contains("program header table lies past"), "{e}");
    assert_eq!(interp_of("/x", &b).unwrap().as_deref(), Some(LOADER));
    // The table in the head, the path past it.
    let b = dyn_elf(64, HEAD_BYTES + 8192);
    let e = interp_of("/x", &b[..head]).unwrap_err();
    assert!(format!("{e}").contains("PT_INTERP path lies past"), "{e}");
    assert_eq!(interp_of("/x", &b).unwrap().as_deref(), Some(LOADER));
    // Both in the head: the head answers as the whole file does.
    let b = dyn_elf(64, 512);
    assert_eq!(interp_of("/x", &b[..head.min(b.len())]).unwrap().as_deref(), Some(LOADER));
}

/// A program header table over the bound of the kernel, or with entries
/// smaller than a program header, is refused by name.
#[test]
fn a_hostile_program_header_table_is_refused() {
    let mut b = dyn_elf(64, 512);
    b[56..58].copy_from_slice(&u16::MAX.to_le_bytes());
    let e = interp_of("/x", &b).unwrap_err();
    assert!(format!("{e}").contains(&format!("over the {PHDR_TABLE_MAX}")), "{e}");
    let mut b = dyn_elf(64, 512);
    b[54..56].copy_from_slice(&8u16.to_le_bytes());
    let e = interp_of("/x", &b).unwrap_err();
    assert!(format!("{e}").contains("e_phentsize 8"), "{e}");
}

/// The bounded read of a file and the parse of the same bytes in hand
/// give the same answer, on this test binary.
#[test]
fn the_bounded_read_agrees_with_the_parse_of_the_whole_file() {
    let me = std::env::current_exe().unwrap();
    let me = me.to_str().unwrap();
    let read = Elf::read(me).unwrap();
    let parsed = Elf::parse(me, &std::fs::read(me).unwrap()).unwrap();
    assert_eq!(format!("{read:?}"), format!("{parsed:?}"));
    assert_eq!(interp_of_file(me).unwrap(), read.interp);
}

const LOADER: &str = "/lib64/ld-linux-x86-64.so.2";

/// A 64-bit little-endian file with a `PT_LOAD` and a `PT_INTERP`: the
/// table at `phoff`, the path of [`LOADER`] at `interp_at`.
fn dyn_elf(phoff: u64, interp_at: u64) -> Vec<u8> {
    let at = |v: u64| usize::try_from(v).unwrap();
    let mut b = tiny_elf(62);
    b.resize(at(phoff.max(interp_at)) + 4096, 0);
    b[32..40].copy_from_slice(&phoff.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&2u16.to_le_bytes());
    b[at(phoff)..at(phoff) + 4].copy_from_slice(&1u32.to_le_bytes());
    b[at(phoff) + 56..at(phoff) + 60].copy_from_slice(&3u32.to_le_bytes());
    b[at(phoff) + 64..at(phoff) + 72].copy_from_slice(&interp_at.to_le_bytes());
    b[at(interp_at)..at(interp_at) + LOADER.len()].copy_from_slice(LOADER.as_bytes());
    b
}

// ------------------------------------------------------------ fixtures

pub(super) fn fake(
    path: &str,
    soname: Option<&str>,
    needed: &[&str],
    defined: &[(&str, Option<&str>)],
    declares: &[&str],
) -> Elf {
    Elf {
        path: path.to_string(),
        machine: pod_hostprobe::binfmt::SELF_MACHINE,
        interp: None,
        needed: needed.iter().map(|s| (*s).to_string()).collect(),
        soname: soname.map(str::to_string),
        sections: Vec::new(),
        defined: defined.iter().map(|(n, v)| ver(n, *v)).collect(),
        imported: Vec::new(),
        declares: declares.iter().map(|s| (*s).to_string()).collect(),
    }
}

pub(super) fn fake_import(path: &str, needed: &[&str], imported: &[(&str, Option<&str>)]) -> Elf {
    let mut e = fake(path, None, needed, &[], &[]);
    e.imported = imported.iter().map(|(n, v)| ver(n, *v)).collect();
    e
}

fn ver(name: &str, version: Option<&str>) -> Versioned {
    Versioned { name: name.to_string(), version: version.map(str::to_string), weak: false, file: None }
}
