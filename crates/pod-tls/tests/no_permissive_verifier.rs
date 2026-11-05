//! No crate of this workspace carries a certificate verifier that accepts
//! each certificate, or installs one in a configuration. Such a verifier
//! turns TLS verification off, with no flag and no warning, for whoever
//! installs it. A verifier that prints a chain for a diagnosis lives in an
//! example of a consumer, so no build of a library has it. The shipped
//! configuration installs the WebPKI verifier with
//! `with_custom_certificate_verifier`, which the scan allows. A verifier
//! that pins a single key accepts no other certificate (podssh's for WARP's
//! endpoint, whose chain no public root signs, comes with `pod-warp`): such
//! a file is allowed in [`PINNED`] while it still compares the key, and its
//! own tests fail when the pin is skipped.

use std::path::{Path, PathBuf};

// `ServerCertVerifier for` matches an impl however the trait is named
// (`impl ServerCertVerifier for`, `impl rustls::client::danger::ServerCertVerifier for`).
const FORBIDDEN: [&str; 3] = ["ServerCertVerifier for", "set_certificate_verifier", "ServerCertVerified::assertion"];

/// Each pinning verifier's file, and the comparison with the pinned key that
/// the file must hold for the allowance to stand. None yet: podssh's of
/// WARP (`== Some(self.point.as_slice())`) joins with `pod-warp`.
const PINNED: &[(&str, &str)] = &[];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The source files of each crate, and what of `FORBIDDEN` each one holds.
fn hits() -> (usize, Vec<String>) {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap().flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut found = Vec::new();
    let mut pinned_seen = vec![false; PINNED.len()];
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let name = file.strip_prefix(crates).unwrap_or(file);
        let shown = name.to_string_lossy().replace('\\', "/");
        if let Some(at) = PINNED.iter().position(|(path, _)| shown == *path) {
            pinned_seen[at] = true;
            if text.contains(PINNED[at].1) {
                continue;
            }
            found.push(format!("crates/{shown}: the pin's comparison {:?} is gone", PINNED[at].1));
            continue;
        }
        for pattern in FORBIDDEN {
            if text.contains(pattern) {
                found.push(format!("crates/{shown}: {pattern}"));
            }
        }
    }
    // An allowance for a file that is no longer there is one to remove.
    for (seen, (path, _)) in pinned_seen.iter().zip(PINNED) {
        if !seen {
            found.push(format!("crates/{path}: allowed, but not in the tree"));
        }
    }
    (files.len(), found)
}

#[test]
fn no_crate_accepts_each_certificate() {
    let (files, found) = hits();
    // A scan that reads nothing passes on any tree.
    assert!(files > 60, "the scan read {files} files: is it reading the workspace?");
    assert!(found.is_empty(), "a permissive verifier in the source: {found:#?}");
}
