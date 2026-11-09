use super::*;

const N: Names = Names::DEFAULT;

fn all_up() -> Availability {
    Availability { memfd: true, fuse: true, tmpfs: true, rundir: true, cache: true, cache_requested: true }
}

fn all_down() -> Availability {
    Availability { memfd: false, fuse: false, tmpfs: false, rundir: false, cache: false, cache_requested: false }
}

/// The order is the design: memfd first, cache last and only when asked.
#[test]
fn the_trial_order_is_memfd_first_and_cache_last() {
    assert_eq!(Mode::ORDER.map(Mode::name), ["memfd", "fuse", "tmpfs", "rundir", "cache"]);
    assert_eq!(choose(&N, None, &all_up(), None).unwrap(), Mode::Memfd);
}

/// A forced mode that is down refuses with its name, and does not fall
/// through to a rung that the caller did not ask for.
#[test]
fn a_forced_mode_that_is_down_refuses_rather_than_falling_through() {
    let e = choose(&N, Some(Mode::Fuse), &all_down(), None).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("fuse"), "{text}");
    assert!(text.contains("/dev/fuse"), "{text}");
    assert!(text.contains(N.mode_request), "{text}");
    assert_eq!(e.payload_code(), None);
}

/// A forced mode that is up is honoured, also past an earlier rung.
#[test]
fn a_forced_mode_that_is_up_is_honoured() {
    assert_eq!(choose(&N, Some(Mode::Tmpfs), &all_up(), None).unwrap(), Mode::Tmpfs);
}

/// An unknown request refuses with the list, never a guess.
#[test]
fn an_unknown_request_refuses_with_the_list() {
    let e = Mode::parse(&N, "memfd2").unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("memfd2"), "{text}");
    assert!(text.contains("rundir"), "{text}");
    assert_eq!(e.payload_code(), None);
}

/// `run-dir` and `run_dir` spell the same rung as `rundir`.
#[test]
fn the_run_dir_spellings_agree() {
    assert_eq!(Mode::parse(&N, "run-dir").unwrap(), Mode::RunDir);
    assert_eq!(Mode::parse(&N, "run_dir").unwrap(), Mode::RunDir);
    assert_eq!(Mode::parse(&N, "rundir").unwrap(), Mode::RunDir);
}

/// The cache runs only when asked for: a machine that can cache, with no
/// request, skips it and does not fill the disk.
#[test]
fn the_cache_runs_only_when_asked_for() {
    let mut avail = all_down();
    avail.cache = true;
    let e = choose(&N, None, &avail, None).unwrap_err();
    assert!(format!("{e}").contains(&format!("{}=1", N.cache)), "{e}");

    avail.cache_requested = true;
    assert_eq!(choose(&N, None, &avail, None).unwrap(), Mode::Cache);
}

/// With nothing up and nothing asked, the refusal names each rung.
#[test]
fn nothing_up_is_a_refusal_naming_every_rung() {
    let e = choose(&N, None, &all_down(), None).unwrap_err();
    let text = format!("{e}");
    for rung in ["memfd", "/dev/fuse", "tmpfs", "run-directory", "cache"] {
        assert!(text.contains(rung), "{text}");
    }
}

/// The memfd refusal carries the answer of the eligibility check.
#[test]
fn a_forced_memfd_names_why_the_payload_cannot_go_that_way() {
    let why = MemfdRefusal::HasInterp("/lib64/ld-linux-x86-64.so.2".to_string());
    let e = choose(&N, Some(Mode::Memfd), &all_down(), Some(&why)).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains("ld-linux"), "{text}");
}

/// A refusal is a failure of the entry, whose code the caller gives
/// (docker's 125): [`Error::code_or`] is the one place that maps it.
#[test]
fn refusals_are_dockers_runtime_error() {
    let e = choose(&N, Some(Mode::Fuse), &all_down(), None).unwrap_err();
    assert!(matches!(e, Error::Runtime(_)), "{e:?}");
    assert_eq!(e.code_or(125), 125);
}

fn fixture_root(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pod-enter-ladder-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

/// A bare name is found along `path_dirs`, in order, and an absolute
/// argument names the path under the root file system.
#[test]
fn a_payload_resolves_along_path_dirs_and_by_path() {
    let d = fixture_root("resolve");
    std::fs::write(d.join("bin/prog"), b"\x7fELF").unwrap();
    let root = d.to_string_lossy().to_string();
    let dirs = ["/sbin", "/bin"].iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    let got = resolve_payload(&root, "prog", &dirs).unwrap();
    assert!(got.ends_with("bin/prog"), "{got}");
    let got = resolve_payload(&root, "/bin/prog", &dirs).unwrap();
    assert!(got.ends_with("bin/prog"), "{got}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A name that the image does not hold is a sentence.
#[test]
fn a_missing_payload_is_a_sentence() {
    let d = fixture_root("missing");
    let root = d.to_string_lossy().to_string();
    let e = resolve_payload(&root, "absent", &["/bin".to_string()]).unwrap_err();
    assert!(e.to_string().contains("names no file"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A `..` that escapes the root is refused, not followed: the path that it
/// names is a path of the host.
#[test]
fn a_payload_escaping_the_image_is_refused() {
    let d = fixture_root("escape");
    std::os::unix::fs::symlink("../../outside", d.join("bin/evil")).unwrap();
    let root = d.to_string_lossy().to_string();
    let e = resolve_payload(&root, "evil", &["/bin".to_string()]).unwrap_err();
    assert!(e.to_string().contains("escapes the image"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The bytes are what the file holds, whole.
#[test]
fn the_payload_bytes_are_the_files() {
    let d = fixture_root("bytes");
    std::fs::write(d.join("bin/prog"), b"#!/bin/sh\necho hi\n").unwrap();
    let root = d.to_string_lossy().to_string();
    let got = payload_bytes(&root, "prog", &["/bin".to_string()]).unwrap();
    assert_eq!(got, b"#!/bin/sh\necho hi\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// The guest path is the resolved one, not the invoked one: a `/bin` link
/// of a merged `/usr` resolves through it.
#[test]
fn the_guest_exe_is_the_resolved_guest_path() {
    let d = fixture_root("exe");
    std::fs::create_dir_all(d.join("usr/bin")).unwrap();
    std::fs::write(d.join("usr/bin/bash"), b"\x7fELF").unwrap();
    // The link replaces the plain directory of the fixture: a symlink
    // call fails where a directory is.
    std::fs::remove_dir(d.join("bin")).unwrap();
    std::os::unix::fs::symlink("usr/bin", d.join("bin")).unwrap();
    let root = d.to_string_lossy().to_string();
    let dirs = ["/bin".to_string()];
    assert_eq!(guest_exe(&root, "bash", &dirs).as_deref(), Some("/usr/bin/bash"));
    assert_eq!(guest_exe(&root, "/bin/bash", &dirs).as_deref(), Some("/usr/bin/bash"));
    assert_eq!(guest_exe(&root, "absent", &dirs), None);
    let _ = std::fs::remove_dir_all(&d);
}
