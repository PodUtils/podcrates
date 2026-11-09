use super::*;
use std::os::unix::process::CommandExt;

/// T-0414: `resolve_in` never lists a directory it was handed.
///
/// The orchestrator half stages a rootfs whose directories deny
/// listing but allow traversal (mode 111), then re-runs the worker
/// half below as `nobody` (uid and gid 65534). The worker first
/// proves the fixture bites (a listing fails `EACCES`), then
/// resolves a payload by name, which must succeed: a resolver that
/// listed anywhere would die where the worker succeeds.
///
/// Three outcomes, each honest: `EPERM` on the spawn means this
/// host cannot shed privilege, so the test records that and passes
/// nothing; a green child proved the property on this machine; any
/// other child failure fails, including a worker that finds itself
/// still privileged (which would otherwise pass vacuously).
#[test]
fn resolve_past_a_denied_listing() {
    let d = std::env::temp_dir().join(format!("podvm-enter-nolist-{}", std::process::id()));
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
        .env("PODVM_TEST_NOBODY_ROOT", &d)
        .env("PODVM_TEST_NOBODY_GUEST", "bin/prog")
        .current_dir(std::env::temp_dir())
        .uid(65534)
        .gid(65534);
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.raw_os_error() == Some(libc_eperm()) => {
            eprintln!(
                "SKIP resolve_past_a_denied_listing: this host cannot run a \
                     child as nobody ({e}), so the denied listing cannot be staged here"
            );
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

/// `EPERM` without the `libc` crate: this crate holds no dependency
/// for one number, and the number is the kernel's, identical on
/// every Linux architecture this runtime stands on.
fn libc_eperm() -> i32 {
    1
}

/// T-0414: the worker half of [`resolve_past_a_denied_listing`].
///
/// Runs only where that test spawned it (the marker environment is
/// absent under a plain `cargo test`, where it returns at once). A
/// listing that succeeds here means the process is still
/// privileged, which would prove nothing: it panics rather than
/// passing vacuously.
#[test]
fn worker_resolves_past_a_denied_listing() {
    let Ok(root) = std::env::var("PODVM_TEST_NOBODY_ROOT") else {
        return;
    };
    let guest =
        std::env::var("PODVM_TEST_NOBODY_GUEST").expect("the orchestrator sets both halves");
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

/// ⭐ Check A of `experiments/results/interposer-abi.txt`, as a predicate.
#[test]
fn the_soname_discriminator_is_the_one_that_was_measured() {
    assert_eq!(Flavour::of_libc_name("libc.so.6"), Flavour::Glibc);
    assert_eq!(Flavour::of_libc_name("libc.so"), Flavour::Musl);
    assert_eq!(
        Flavour::of_libc_name("/lib/ld-musl-x86_64.so.1"),
        Flavour::Musl
    );
    assert_eq!(
        Flavour::of_libc_name("/lib/libc.musl-aarch64.so.1"),
        Flavour::Musl
    );
    assert_eq!(
        Flavour::of_libc_name("/lib64/ld-linux-x86-64.so.2"),
        Flavour::Glibc
    );
    // ⛔ And a third state for anything else, never a guess.
    assert_eq!(Flavour::of_libc_name("libz.so.1"), Flavour::Unknown);
    assert_eq!(
        Flavour::of_needed(&["libgcc_s.so.1".into(), "libc.so.6".into()]),
        Flavour::Glibc
    );
}

/// ⛔ The reader reads THIS binary, which is the only ELF file every host
/// running this test is guaranteed to have. ⚠ It is static-pie under the
/// workspace's own `+crt-static`, so it has no `PT_INTERP` and no
/// `DT_NEEDED`: what is asserted is that the header and the section table
/// parse and that the machine is this architecture's.
#[test]
fn this_binarys_own_header_parses() {
    let me = std::env::current_exe().unwrap();
    let got = Elf::read(me.to_str().unwrap()).unwrap();
    assert_eq!(got.machine, podvm_probe::binfmt::SELF_MACHINE);
    // ⭐ The section names T-0706 classifies Go payloads by. A binary with
    // no readable section table gives the classifier nothing to match, so
    // the parse the classifier trusts is asserted here.
    assert!(
        !got.sections.is_empty(),
        "this binary parsed with no section names"
    );
}

/// ⛔ **The `.symtab` trap, as an assertion rather than a comment.** A
/// shipped `libc.so.6` has 0 defined symbols in `.symtab` and 3136 in
/// `.dynsym` (`experiments/results/interposer-abi.txt` check D), so a reader
/// asking `.symtab` refuses every artefact and reads exactly like a check
/// that works. ⚠ This runs only where a system libc is present; where it is
/// not, the assertion is skipped rather than passed, because "could not run"
/// is a third state.
#[test]
fn a_shipped_libc_defines_its_symbols_in_dynsym() {
    let Some(p) = [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "/usr/lib/libc.so.6",
        "/lib/ld-musl-x86_64.so.1",
    ]
    .into_iter()
    .find(|p| std::path::Path::new(p).exists()) else {
        eprintln!("SKIP: this host ships no libc at a conventional path");
        return;
    };
    let libc = Elf::read(p).unwrap();
    assert!(
        libc.defined.len() > 100,
        "{p} defined only {} symbols; .symtab was read instead of .dynsym",
        libc.defined.len()
    );
    assert!(libc.defines(&Versioned {
        name: "open".into(),
        version: None,
        weak: false,
        file: None,
    }));
    assert_ne!(libc.flavour(), Flavour::Unknown, "{p}");
}

/// ⭐ Check C, as the predicate rather than as a run: an import at a version
/// the libc does not declare is REFUSED, and the refusal names the version
/// the loader would have named.
#[test]
fn an_import_at_a_version_the_libc_does_not_declare_is_refused() {
    let libc = fake(
        "/lib/libc.so.6",
        Some("libc.so.6"),
        &[],
        &[("open", None), ("snprintf", Some("GLIBC_2.4"))],
        &["GLIBC_2.4", "GLIBC_2.31"],
    );
    let obj = fake_import(
        "/i.so",
        &["libc.so.6"],
        &[("open", None), ("snprintf", Some("GLIBC_2.34"))],
    );
    let v = admits(&obj, &libc);
    let Verdict::Refused(why) = v else {
        panic!("admitted an import at GLIBC_2.34 against a libc declaring 2.31");
    };
    assert!(why.contains("GLIBC_2.34"), "{why}");
    assert!(why.contains("GLIBC_2.31"), "{why}");
}

/// ⛔ The coarse guard T-0709 asks for by name: a musl rootfs handed a glibc
/// object says so in those words, not through a relocation error.
#[test]
fn a_glibc_object_against_a_musl_rootfs_is_refused_in_those_words() {
    let libc = fake(
        "/lib/ld-musl-x86_64.so.1",
        None,
        &[],
        &[("open", None)],
        &[],
    );
    let obj = fake_import("/i.so", &["libc.so.6"], &[("open", None)]);
    let Verdict::Refused(why) = admits(&obj, &libc) else {
        panic!("a glibc object was admitted into a musl rootfs");
    };
    assert!(why.contains("musl"), "{why}");
    assert!(why.contains("glibc"), "{why}");
}

/// ⚠ And the matching pair is ADMITTED, or the check would be a refusal
/// rather than a gate.
#[test]
fn a_matching_object_and_libc_are_admitted() {
    let libc = fake(
        "/lib/libc.so.6",
        Some("libc.so.6"),
        &[],
        &[("open", None), ("snprintf", Some("GLIBC_2.4"))],
        &["GLIBC_2.4"],
    );
    let obj = fake_import(
        "/i.so",
        &["libc.so.6"],
        &[("open", None), ("snprintf", Some("GLIBC_2.4"))],
    );
    assert_eq!(admits(&obj, &libc), Verdict::Admitted);
}

/// ⛔ A symbol the libc does not define at all is refused before the version
/// question is asked, and the message names the symbol.
#[test]
fn an_undefined_symbol_is_refused_by_name() {
    let libc = fake(
        "/lib/libc.so.6",
        Some("libc.so.6"),
        &[],
        &[("open", None)],
        &[],
    );
    let obj = fake_import("/i.so", &["libc.so.6"], &[("__snprintf_chk", None)]);
    let Verdict::Refused(why) = admits(&obj, &libc) else {
        panic!("admitted an object importing a symbol the libc does not define");
    };
    assert!(why.contains("__snprintf_chk"), "{why}");
    assert!(why.contains(".dynsym"), "{why}");
}

/// ⚠ The libc is derived from the payload's own interpreter rather than
/// looked up in a table of distributions.
#[test]
fn the_libc_is_found_beside_the_interpreter() {
    let d = std::env::temp_dir().join(format!("podvm-abi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lib/x86_64-linux-gnu")).unwrap();
    std::fs::write(d.join("lib/x86_64-linux-gnu/libc.so.6"), b"x").unwrap();
    std::fs::write(
        d.join("lib/ld-musl-x86_64.so.1"),
        tiny_elf(podvm_probe::binfmt::SELF_MACHINE),
    )
    .unwrap();
    let root = d.to_string_lossy().to_string();
    let m = podvm_probe::binfmt::SELF_MACHINE;
    assert_eq!(
        libc_beside(&root, "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", m),
        Some(format!("{root}/lib/x86_64-linux-gnu/libc.so.6"))
    );
    // ⭐ On musl the interpreter IS the C library: the verbatim path
    // parses, so it is returned with no search.
    assert_eq!(
        libc_beside(&root, "/lib/ld-musl-x86_64.so.1", m),
        Some(format!("{root}/lib/ld-musl-x86_64.so.1"))
    );
    // ⛔ And an interpreter podvm does not recognise gets no answer rather
    // than a guessed one.
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

/// ⭐ The multiarch layout, where the loader's own directory holds no
/// libc: Debian keeps the loader in `/lib64` and the library in
/// `/lib/<triplet>/`. The fallback search finds it by name and machine,
/// and a wrong-machine decoy does not win.
#[test]
fn the_libc_is_found_across_a_multiarch_split() {
    let d = std::env::temp_dir().join(format!("podvm-abi64-{}", std::process::id()));
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
    // ⛔ The loader's own directory has no libc, so the direct rule
    // misses and the search must find the machine-matching one.
    assert_eq!(
        libc_beside(&root, "/lib64/ld-linux-x86-64.so.2", ME),
        Some(format!("{root}/lib/x86_64-linux-gnu/libc.so.6"))
    );
    // ⛔ And a machine with no library here gets no answer, not the
    // other architecture's.
    assert_eq!(libc_beside(&root, "/lib64/ld-linux-x86-64.so.2", 40), None);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⭐ The musl loader installed anywhere else: one matrix row keeps it
/// out of `/lib`, and the verbatim path then names nothing. The search
/// finds it by basename, which carries the architecture already.
#[test]
fn a_musl_loader_outside_lib_is_found_by_name() {
    let d = std::env::temp_dir().join(format!("podvm-abi-musl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("usr/lib")).unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("usr/lib/ld-musl-x86_64.so.1"), b"fake-libc").unwrap();
    let root = d.to_string_lossy().to_string();
    let m = podvm_probe::binfmt::SELF_MACHINE;
    assert_eq!(
        libc_beside(&root, "/lib/ld-musl-x86_64.so.1", m),
        Some(format!("{root}/usr/lib/ld-musl-x86_64.so.1"))
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_newest_declared_version_is_reported_numerically() {
    // ⛔ Not lexically over the whole string: `GLIBC_2.9` sorts after
    // `GLIBC_2.34` as text and is older.
    let v: Vec<String> = ["GLIBC_2.4", "GLIBC_2.34", "GLIBC_2.9"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(newest(&v), "GLIBC_2.34");
}

#[test]
fn a_file_that_is_not_elf_is_a_named_refusal_and_not_a_panic() {
    let e = Elf::parse("/x", b"#!/bin/sh\necho hi\n").unwrap_err();
    assert!(format!("{e}").contains("not an ELF file"), "{e}");
    // ⛔ And a truncated one, which is the shape a hostile image takes.
    let e = Elf::parse("/x", b"\x7fELF\x02\x01").unwrap_err();
    assert!(format!("{e}").contains("not an ELF file"), "{e}");
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
        machine: podvm_probe::binfmt::SELF_MACHINE,
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
    Versioned {
        name: name.to_string(),
        version: version.map(str::to_string),
        weak: false,
        file: None,
    }
}
