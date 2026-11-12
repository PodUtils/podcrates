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
//! own tests fail when the pin is skipped. `pod-warp` holds that verifier.
//!
//! `dangerous()` of rustls opens `with_custom_certificate_verifier`. Only the
//! files of [`DANGEROUS`] call it, and each must still build the WebPKI
//! verifier that it installs: a new file that calls it, and an allowance for
//! a file that no longer calls it, fail the scan. A consumer keeps its own
//! scan of its own tree (podnet: its origin option noTLSVerify, its MASQUE
//! key pin and its doctor).

use std::path::{Path, PathBuf};

// `ServerCertVerifier for` matches an impl however the trait is named
// (`impl ServerCertVerifier for`, `impl rustls::client::danger::ServerCertVerifier for`).
const FORBIDDEN: [&str; 3] = ["ServerCertVerifier for", "set_certificate_verifier", "ServerCertVerified::assertion"];

/// Each pinning verifier's file, and the comparison with the pinned key that
/// the file must hold for the allowance to stand: the mutual TLS of pinned
/// keys (T-165), whose `tests/mutual.rs` fails when the pin is skipped.
/// The pin of WARP's front door in `pod-warp` (T-028): its `tests/net.rs`
/// fails when the pin is skipped (plant `warp-pin-unchecked`).
const PINNED: &[(&str, &str)] =
    &[("pod-tls/src/mutual.rs", "self.pins.holds(&pin)"), ("pod-warp/src/tls.rs", "if got != self.expected")];

/// Each file that calls `dangerous()`, and the build of the WebPKI verifier
/// (or, for a file of [`PINNED`], the comparison with the pin) that the
/// file must hold for the allowance to stand.
const DANGEROUS: &[(&str, &str)] = &[
    ("pod-tls/src/context.rs", "WebPkiServerVerifier::builder_with_provider"),
    ("pod-tls/src/ech/config.rs", "WebPkiServerVerifier::builder_with_provider"),
    ("pod-tls/src/mutual.rs", "self.pins.holds(&pin)"),
    ("pod-warp/src/tls.rs", "if got != self.expected"),
];

/// What the scan read and found.
struct Scan {
    /// The number of files read.
    files: usize,
    /// Each permissive verifier, and each pin whose comparison is gone.
    found: Vec<String>,
    /// Each file that calls `dangerous()`, and each allowed one whose WebPKI
    /// verifier is gone.
    dangerous: Vec<String>,
}

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

/// The source files of each crate, and what of `FORBIDDEN` and `dangerous()`
/// each one holds.
fn hits() -> Scan {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap().flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut found = Vec::new();
    let mut dangerous = Vec::new();
    let marker = ["dangerous", "()"].concat();
    let mut pinned_seen = vec![false; PINNED.len()];
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let name = file.strip_prefix(crates).unwrap_or(file);
        let shown = name.to_string_lossy().replace('\\', "/");
        if text.contains(&marker) {
            dangerous.push(format!("crates/{shown}"));
            if let Some((_, webpki)) = DANGEROUS.iter().find(|(path, _)| shown == *path)
                && !text.contains(webpki)
            {
                dangerous.push(format!("crates/{shown}: the WebPKI verifier {webpki:?} is gone"));
            }
        }
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
    dangerous.sort();
    Scan { files: files.len(), found, dangerous }
}

#[test]
fn no_crate_accepts_each_certificate() {
    let Scan { files, found, .. } = hits();
    // A scan that reads nothing passes on any tree.
    assert!(files > 60, "the scan read {files} files: is it reading the workspace?");
    assert!(found.is_empty(), "a permissive verifier in the source: {found:#?}");
}

/// Only the files of `DANGEROUS` call `dangerous()`, each with its WebPKI
/// verifier, and each of them still calls it.
#[test]
fn no_other_code_sets_its_own_verifier() {
    let Scan { files, dangerous, .. } = hits();
    assert!(files > 60, "the scan read {files} files: is it reading the workspace?");
    let mut allowed: Vec<String> = DANGEROUS.iter().map(|(path, _)| format!("crates/{path}")).collect();
    allowed.sort();
    assert_eq!(dangerous, allowed, "the files that call dangerous() of rustls");
}
