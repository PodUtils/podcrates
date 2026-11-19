//! An update and a rollback with shell scripts in the place of a binary
//! (podnet's `install_keeps_the_binary_before_and_rollback_swaps`,
//! `a_bad_new_binary_changes_nothing` and
//! `rollback_needs_a_kept_binary_that_runs`). Unix only: a script runs by
//! its `#!` line.

#![cfg(unix)]

use std::time::Duration;

use super::*;
use crate::replace::{Stopped, replace};
use crate::version::Version;

/// A shell script in the place of a podnet binary.
fn fake(version_line: &str, extra: &str) -> Vec<u8> {
    format!("#!/bin/sh\n{extra}\necho '{version_line}'\n").into_bytes()
}

fn put(path: &Path, body: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn version_of(path: &Path) -> String {
    let out = std::process::Command::new(path).arg("--version").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn expect<'a>(version: Option<&'a Version>, limit: Duration) -> Expect<'a> {
    Expect { program: "podnet", version, limit }
}

/// The trial of an update to `version`.
fn tried(version: &Version, limit: Duration) -> impl Fn(&Path) -> Result<String, String> + '_ {
    move |file: &Path| trial::check(file, &expect(Some(version), limit))
}

/// An update keeps the binary before as podnet.prev; a rollback puts it
/// back, and a second rollback goes forward again.
#[test]
fn install_keeps_the_binary_before_and_rollback_swaps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("podnet");
    put(&path, &fake("podnet 0.3.0-beta.4 (old)", ""));
    let new = Version::parse("0.3.0-beta.5").unwrap();
    let replaced = replace(&path, &fake("podnet 0.3.0-beta.5 (new)", ""), &tried(&new, trial::LIMIT)).unwrap();
    assert_eq!(replaced.said, "podnet 0.3.0-beta.5 (new)");
    assert_eq!(version_of(&path), "podnet 0.3.0-beta.5 (new)");
    assert_eq!(version_of(&prev_of(&path)), "podnet 0.3.0-beta.4 (old)");
    let any = expect(None, trial::LIMIT);
    assert_eq!(fallback(&path, &any), Some(prev_of(&path)));
    let back = rollback(&path, &any).unwrap();
    assert_eq!(back.said, "podnet 0.3.0-beta.4 (old)");
    assert_eq!(back.kept, Ok(prev_of(&path)));
    assert_eq!(version_of(&path), "podnet 0.3.0-beta.4 (old)");
    assert_eq!(version_of(&prev_of(&path)), "podnet 0.3.0-beta.5 (new)");
    assert_eq!(rollback(&path, &any).unwrap().said, "podnet 0.3.0-beta.5 (new)");
    let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 2, "no temporary file stays: {names:?}");
}

/// The trial refuses a binary that exits with an error, that prints
/// another version or another name, or that does not answer in time. The
/// binary in place does not change, and no file stays.
#[test]
fn a_bad_new_binary_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("podnet");
    put(&path, &fake("podnet 0.3.0-beta.4 (old)", ""));
    let new = Version::parse("0.3.0-beta.5").unwrap();
    let cases = [
        (fake("podnet 0.3.0-beta.5 (new)", "echo 'no libc' >&2; exit 3"), "exit status: 3: no libc"),
        ([fake("podnet 0.3.0-beta.5 (new)", ""), b"exit 1\n".to_vec()].concat(), "exit status: 1"),
        (fake("podnet 0.2.0 (other)", ""), "it is podnet 0.2.0, not 0.3.0-beta.5"),
        (fake("podnetx 0.3.0-beta.5", ""), "not podnet VERSION"),
        (fake("podnet 0.3.0-beta.5 (new)", "sleep 30"), "did not end within 500 ms"),
    ];
    for (body, words) in cases {
        match replace(&path, &body, &tried(&new, Duration::from_millis(500))) {
            Err(Stopped::Trial(why)) => assert!(why.contains(words), "{why}"),
            other => panic!("refused by its trial, not {other:?}"),
        }
        assert_eq!(version_of(&path), "podnet 0.3.0-beta.4 (old)");
        assert!(!prev_of(&path).exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

/// A rollback with no kept binary, or with a kept binary that fails its
/// trial, changes nothing.
#[test]
fn rollback_needs_a_kept_binary_that_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("podnet");
    put(&path, &fake("podnet 0.3.0-beta.5 (new)", ""));
    let any = expect(None, Duration::from_millis(500));
    match rollback(&path, &any) {
        Err(Error::NotFound(why)) => assert!(why.contains("no update kept a binary"), "{why}"),
        other => panic!("{other:?}"),
    }
    put(&prev_of(&path), &fake("podnet 0.3.0-beta.4 (old)", "exit 2"));
    assert!(matches!(rollback(&path, &any), Err(Error::Refused(_))));
    assert_eq!(fallback(&path, &any), None);
    assert_eq!(version_of(&path), "podnet 0.3.0-beta.5 (new)");
}
