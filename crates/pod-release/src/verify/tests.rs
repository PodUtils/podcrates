//! The files of podcrates v0.76.0 as its release published them:
//! `SHA256SUMS`, its bundle and `podlab-install.sh`. The verifier takes
//! them; one changed byte of a file, of `SHA256SUMS` or of the bundle, and
//! another identity, each fail.

use std::path::{Path, PathBuf};

use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.76.0")
}

/// The file of the fixtures that holds the asset `name`: the installer
/// is kept as `.bytes`, so that no check of the scripts reads it.
fn stored(name: &str) -> PathBuf {
    let file = if name == "podlab-install.sh" { "podlab-install.sh.bytes" } else { name };
    fixtures().join(file)
}

fn who() -> Identity {
    Identity::github("PodUtils/podcrates", "release.yml", "v0.76.0")
}

/// A copy of the fixtures in a new directory, with `change` applied.
fn copy(change: impl Fn(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in [SUMS, BUNDLE, "podlab-install.sh"] {
        std::fs::copy(stored(name), dir.path().join(name)).unwrap();
    }
    change(dir.path());
    dir
}

/// One bit of the byte `at` of the file `name` in `dir` changed.
fn flip(dir: &Path, name: &str, at: usize) {
    let path = dir.join(name);
    let mut bytes = std::fs::read(&path).unwrap();
    let at = at.min(bytes.len() - 1);
    bytes[at] ^= 1;
    std::fs::write(&path, bytes).unwrap();
}

#[test]
fn the_published_release_verifies() {
    let dir = copy(|_| {});
    let checked = verify_dir(dir.path(), &who(), &Roots::sigstore()).expect("v0.76.0 verifies");
    assert_eq!(checked.files, ["podlab-install.sh"]);
    assert_eq!(checked.missing.len(), 7, "{:?}", checked.missing);
    assert!(checked.index.is_none());
    assert_eq!(checked.verified.logged.index, 3992748924);
    assert!(checked.to_string().contains("1 files checked, 7 missing"), "{checked}");
}

#[test]
fn one_changed_byte_of_a_file_fails() {
    for at in [0, 15000, usize::MAX] {
        let dir = copy(|d| flip(d, "podlab-install.sh", at));
        match verify_dir(dir.path(), &who(), &Roots::sigstore()) {
            Err(Error::Refused(why)) => assert!(why.contains("the SHA-256 of podlab-install.sh is"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn one_changed_byte_of_the_sums_fails() {
    for at in [0, 400, usize::MAX] {
        let dir = copy(|d| flip(d, SUMS, at));
        match verify_dir(dir.path(), &who(), &Roots::sigstore()) {
            Err(Error::Refused(why)) => assert!(why.contains("the signature of SHA256SUMS"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}

/// A byte of the signature's value in the bundle: the first character of
/// its base64 text, changed.
#[test]
fn one_changed_byte_of_the_bundle_fails() {
    let text = std::fs::read_to_string(fixtures().join(BUNDLE)).unwrap();
    let at = text.find("\"signature\":\"").unwrap() + "\"signature\":\"".len();
    let dir = copy(|d| flip(d, BUNDLE, at));
    match verify_dir(dir.path(), &who(), &Roots::sigstore()) {
        Err(Error::Refused(why)) => assert!(why.contains("the signature of SHA256SUMS"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn another_identity_fails() {
    let dir = copy(|_| {});
    for who in [
        Identity::github("PodUtils/podcrates", "release.yml", "v0.76.1"),
        Identity::github("PodUtils/podnet", "release.yml", "v0.76.0"),
        Identity { issuer: "https://gitlab.com".into(), ..who() },
    ] {
        assert!(matches!(verify_dir(dir.path(), &who, &Roots::sigstore()), Err(Error::Refused(_))), "{who:?}");
    }
}

#[test]
fn a_directory_with_no_bundle_is_unavailable() {
    let dir = copy(|d| std::fs::remove_file(d.join(BUNDLE)).unwrap());
    assert!(matches!(verify_dir(dir.path(), &who(), &Roots::sigstore()), Err(Error::Unavailable(_))));
}

/// The check of one file in memory, as an update makes it.
#[test]
fn a_file_in_memory_is_checked_by_its_line() {
    let read = |name: &str| std::fs::read(stored(name)).unwrap();
    let verified = verify_release(&read(SUMS), &read(BUNDLE), &who(), &Roots::sigstore()).unwrap();
    let mut script = read("podlab-install.sh");
    assert_eq!(verified.check("podlab-install.sh", &script), Ok(()));
    script.push(b'\n');
    assert!(matches!(verified.check("podlab-install.sh", &script), Err(Error::Refused(_))));
    assert!(matches!(verified.check("podlab-sparc64", b""), Err(Error::Refused(_))));
}
