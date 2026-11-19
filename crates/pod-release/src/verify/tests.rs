//! The files of podcrates v0.76.0 as its release published them:
//! `SHA256SUMS`, its bundle and `podlab-install.sh`. The verifier takes
//! them; one changed byte of a file, of `SHA256SUMS` or of the bundle, and
//! another identity, each fail.

use std::path::{Path, PathBuf};

use pod_testkit::tamper::flip;

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
        let dir = copy(|d| flip(&d.join("podlab-install.sh"), at));
        match verify_dir(dir.path(), &who(), &Roots::sigstore()) {
            Err(Error::Refused(why)) => assert!(why.contains("the SHA-256 of podlab-install.sh is"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn one_changed_byte_of_the_sums_fails() {
    for at in [0, 400, usize::MAX] {
        let dir = copy(|d| flip(&d.join(SUMS), at));
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
    let dir = copy(|d| flip(&d.join(BUNDLE), at));
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
fn a_directory_with_no_bundle_is_not_found() {
    let dir = copy(|d| std::fs::remove_file(d.join(BUNDLE)).unwrap());
    assert!(matches!(verify_dir(dir.path(), &who(), &Roots::sigstore()), Err(Error::NotFound(_))));
}

/// The files of podcrates v0.77.0: `SHA256SUMS` names the index, which
/// names the signer.
#[test]
fn the_index_of_the_published_release_verifies_with_its_signer() {
    let dir = tempfile::tempdir().unwrap();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.77.0");
    for name in [SUMS, BUNDLE, crate::index::FILE] {
        std::fs::copy(from.join(name), dir.path().join(name)).unwrap();
    }
    let who = Identity::github("PodUtils/podcrates", "release.yml", "v0.77.0");
    let checked = verify_dir(dir.path(), &who, &Roots::sigstore()).expect("v0.77.0 verifies");
    let index = checked.index.expect("the index");
    assert_eq!(
        (index.project.as_str(), index.version.as_str(), index.files.len()),
        ("PodUtils/podcrates", "v0.77.0", 8)
    );
    assert_eq!(checked.files, [crate::index::FILE]);
    let all = verify_files(dir.path(), &who, &Roots::sigstore(), Which::All);
    assert!(matches!(all, Err(Error::NotFound(ref why)) if why.contains("podlab-0.77.0.tar.gz")), "{all:?}");
    // The signer of the bundle and of the index must be the caller's.
    assert_eq!(index.signed_as(&who), Ok(()));
    for other in [
        Identity::github("PodUtils/podcrates", "release.yml", "v0.76.0"),
        Identity::github("PodUtils/podnet", "release.yml", "v0.77.0"),
        Identity { issuer: "https://gitlab.com".into(), ..who.clone() },
    ] {
        assert!(index.signed_as(&other).is_err(), "{other:?}");
    }
    let moved = crate::index::Index { version: "v0.76.0".into(), ..index.clone() };
    assert!(moved.signed_as(&who).unwrap_err().contains("is of PodUtils/podcrates v0.76.0"));
    let renamed = crate::index::Index {
        signature: crate::index::Signature { bundle: "SHA256SUMS.sig".into(), ..index.signature.clone() },
        ..index
    };
    assert!(renamed.signed_as(&who).is_err());
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
