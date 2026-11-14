//! A reason carries no verdict word of its own.
//!
//! Each printer of a reason adds its own label: podvm's `system abi`
//! prints `refused: {why}`, and its interposer tier prints
//! `declined for ...: {why}`. A label inside the reason doubles the verdict,
//! or names a different one (PodUtils/podvm#331).

use super::*;

/// The labels of a printer. None of them is in a reason.
const LABELS: [&str; 3] = ["refused:", "note:", "declined:"];

/// Fail where `why` holds a label of a printer, at its start or after the
/// end of a sentence: a reason states facts, and the caller states the
/// verdict.
pub(in crate::abi) fn assert_unlabeled(why: &str) {
    let low = why.to_ascii_lowercase();
    for label in LABELS {
        assert!(!low.contains(label), "the reason holds the label `{label}`: {why}");
    }
    assert!(!why.contains("  "), "the reason has a gap where a word is missing: {why}");
}

fn libc_glibc() -> Elf {
    fake("/lib/libc.so.6", Some("libc.so.6"), &[], &[("open", None), ("snprintf", Some("GLIBC_2.4"))], &["GLIBC_2.4"])
}

fn refused(v: Verdict) -> String {
    match v {
        Verdict::Refused(why) => why,
        Verdict::Admitted => panic!("admitted where a refusal was expected"),
    }
}

/// A static object (no `DT_NEEDED`) is refused as static, in a sentence
/// with nothing missing: not "names  in DT_NEEDED" and not "built against
/// unknown".
#[test]
fn a_static_object_is_refused_as_static() {
    let obj = fake_import("/bin/podvm", &[], &[]);
    let why = refused(admits(&obj, &libc_glibc()));
    assert!(why.contains("linked statically"), "{why}");
    assert!(why.contains("no library in DT_NEEDED"), "{why}");
    assert!(!why.contains("unknown"), "{why}");
    assert_unlabeled(&why);
}

/// Each refusal of [`admits`] and of its gates has no label of its own.
#[test]
fn no_refusal_of_admits_carries_a_label() {
    let libc = libc_glibc();
    // The architecture.
    let mut obj = fake_import("/i.so", &["libc.so.6"], &[("open", None)]);
    obj.machine ^= 0x7;
    let why = refused(admits(&obj, &libc));
    assert!(why.contains("ELF machine"), "{why}");
    assert_unlabeled(&why);
    // A C library that the reader cannot identify.
    let odd = fake("/lib/libweird.so", None, &[], &[("open", None)], &[]);
    let why = refused(admits(&fake_import("/i.so", &["libc.so.6"], &[]), &odd));
    assert!(why.contains("could not be identified"), "{why}");
    assert_unlabeled(&why);
    // A glibc object in a musl root.
    let musl = fake("/lib/ld-musl-x86_64.so.1", None, &[], &[("open", None)], &[]);
    let why = refused(admits(&fake_import("/i.so", &["libc.so.6"], &[]), &musl));
    assert!(why.contains("built against glibc") && why.contains("carries musl"), "{why}");
    assert_unlabeled(&why);
    // Needs that name no C library.
    let why = refused(admits(&fake_import("/i.so", &["libfoo.so.1"], &[]), &libc));
    assert!(why.contains("libfoo.so.1") && why.contains("no known C library"), "{why}");
    assert!(!why.contains("against unknown"), "{why}");
    assert_unlabeled(&why);
    // A version newer than the libc declares.
    let obj = fake_import("/i.so", &["libc.so.6"], &[("snprintf", Some("GLIBC_2.34"))]);
    let why = refused(admits(&obj, &libc));
    assert!(why.contains("build host newer than the target"), "{why}");
    assert_unlabeled(&why);
    // A symbol that the libc does not define.
    let obj = fake_import("/i.so", &["libc.so.6"], &[("__snprintf_chk", None)]);
    let why = refused(admits(&obj, &libc));
    assert!(why.contains(".dynsym"), "{why}");
    assert_unlabeled(&why);
}

/// The reasons of an object that cannot be read have no label: they reach
/// a refusal of a link set, which its caller labels.
#[test]
fn no_reason_of_an_unreadable_object_carries_a_label() {
    // A section table at the end of the address space.
    let mut b = tiny_elf(pod_hostprobe::binfmt::SELF_MACHINE);
    b[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    b[58..60].copy_from_slice(&64u16.to_le_bytes());
    let why = format!("{}", Elf::parse("/x", &b).unwrap_err());
    assert!(why.contains("no section table") && why.contains("is not preloaded"), "{why}");
    assert_unlabeled(&why);
    // A file whose tables need more than the reader holds.
    let file = std::fs::File::open("/dev/null").unwrap();
    let src = Src::file(&file, 0);
    if let Src::File { over, .. } = &src {
        over.set(true);
    }
    let why = src.trouble().expect("a read past the ceiling is a trouble");
    assert!(why.contains("is not preloaded"), "{why}");
    assert_unlabeled(&why);
}
