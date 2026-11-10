use super::*;

fn scratch(name: &str) -> String {
    let p = std::env::temp_dir().join(format!("podvm-pkg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("etc")).unwrap();
    p.to_string_lossy().to_string()
}

/// ⛔ **THE TESTS THAT SET `$SSL_CERT_FILE` MAY NOT RUN AT THE SAME TIME**,
/// and this is what stops them.
///
/// `cargo test` runs tests in THREADS of one process, and an environment
/// variable belongs to the process. One test's `remove_var` lands in the
/// middle of another's `ca_bundle`, which then reads THIS MACHINE's real
/// bundle -- this development container announces one -- and asserts on it.
/// ⚠ The same shape as `podvm-image`'s two fork tests, which serialise for
/// the same reason with a different shared thing.
static ANNOUNCED_BUNDLE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// ⚠ The first word, not a substring. An already-commented line must not be
/// counted or commented twice.
#[test]
fn comment_out_matches_the_key_and_not_a_substring() {
    let src = "\
[options]
#DownloadUser = alpm
DownloadUser = alpm
DownloadUserAgent = x
  DownloadUser=alpm
";
    let (got, hits) = comment_out(src, "DownloadUser");
    assert_eq!(hits, 2, "{got}");
    assert!(
        got.contains("# podvm (T-0406): DownloadUser = alpm\n"),
        "{got}"
    );
    assert!(got.contains("DownloadUserAgent = x\n"), "{got}");
    assert!(!got.contains("# podvm (T-0406): #DownloadUser"), "{got}");
}

#[test]
fn pacman_conf_absent_is_skipped_rather_than_failed() {
    let d = scratch("nopacman");
    let root = Root::open(&d).unwrap();
    let (fs, steps) = pacman(&root, &Options::default()).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(!fs[0].degraded);
    // ⛔ And no step: a rootfs that is not Arch-family has no keyring to
    // build, and a step announced on the banner for it would be podvm
    // running `pacman-key` in an image with no pacman.
    assert!(steps.is_empty(), "{steps:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_apt_drop_in_is_idempotent() {
    let d = scratch("apt");
    std::fs::create_dir_all(format!("{d}/etc/apt")).unwrap();
    let root = Root::open(&d).unwrap();
    assert_eq!(
        apt(&root, &Options::default()).unwrap()[0].action,
        Action::Created
    );
    assert_eq!(
        apt(&root, &Options::default()).unwrap()[0].action,
        Action::Unchanged
    );
    let got = std::fs::read_to_string(format!("{d}/{APT_DROP_IN}")).unwrap();
    assert!(got.contains("APT::Sandbox::User \"root\";"), "{got}");
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ A rootfs with no CA bundle gets one, or podvm says it could not.
/// ⚠ Whichever arm this host takes, the rule asserted is the same: podvm
/// either installs a real bundle or says so, and never turns verification
/// off.
#[test]
fn a_rootfs_with_no_ca_bundle_gets_one_or_is_told_why_not() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let d = scratch("ca");
    let root = Root::open(&d).unwrap();
    let fs = ca_bundle(&root, "T-0407", &Options::default()).unwrap();
    let ca = &fs[0];
    assert!(
        matches!(ca.action, Action::Created | Action::Skipped),
        "{ca:?}"
    );
    assert!(!ca.detail.contains("verify=false"), "{ca:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// ⭐ The append is idempotent and it KEEPS the image's own roots. A second
/// run must not grow the file, which is what a byte comparison instead of a
/// marker would have done.
#[test]
fn an_announced_host_bundle_is_appended_once_and_the_image_roots_survive() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let d = scratch("caappend");
    std::fs::create_dir_all(format!("{d}/etc/ssl/certs")).unwrap();
    std::fs::write(
        format!("{d}/etc/ssl/certs/ca-certificates.crt"),
        "# THE IMAGE OWN ROOTS\n",
    )
    .unwrap();
    // ⚠ A real PEM, because `host_bundle_bytes` returns a path only where it
    // parses as at least one certificate.
    let host = std::env::temp_dir().join(format!("podvm-hostca-{}.pem", std::process::id()));
    std::fs::write(&host, SELF_SIGNED_PEM).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::set_var("SSL_CERT_FILE", &host) };

    let root = Root::open(&d).unwrap();
    let first = ca_bundle(&root, "T-0407", &Options::default()).unwrap();
    let second = ca_bundle(&root, "T-0407", &Options::default()).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };

    let got = std::fs::read_to_string(format!("{d}/etc/ssl/certs/ca-certificates.crt")).unwrap();
    assert_eq!(first[0].action, Action::Rewrote, "{:?}", first[0]);
    assert_eq!(second[0].action, Action::Unchanged, "{:?}", second[0]);
    assert!(
        got.contains("# THE IMAGE OWN ROOTS"),
        "the image roots were dropped"
    );
    assert_eq!(got.matches(HOST_CA_MARKER).count(), 1, "appended twice");
    assert!(first[0].degraded, "a trust change is a degradation");
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ `--no-host-cas` refuses, and says what the caller gave up.
#[test]
fn no_host_cas_leaves_the_image_trust_store_byte_identical() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let d = scratch("canohost");
    std::fs::create_dir_all(format!("{d}/etc/ssl/certs")).unwrap();
    let p = format!("{d}/etc/ssl/certs/ca-certificates.crt");
    std::fs::write(&p, "# THE IMAGE OWN ROOTS\n").unwrap();
    let host = std::env::temp_dir().join(format!("podvm-hostca2-{}.pem", std::process::id()));
    std::fs::write(&host, SELF_SIGNED_PEM).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::set_var("SSL_CERT_FILE", &host) };
    let root = Root::open(&d).unwrap();
    let fs = ca_bundle(
        &root,
        "T-0407",
        &Options {
            host_cas: false,
            ..Options::default()
        },
    )
    .unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(fs[0].degraded);
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "# THE IMAGE OWN ROOTS\n"
    );
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

// ------------------------------------------------------------ T-0412

/// ⛔ Exactly one certificate per file, which is what `openssl rehash`
/// requires: measured on 2026-09-09, a file holding three of them is
/// refused with `it does not contain exactly one certificate or CRL` and
/// NOTHING in the directory is linked.
#[test]
fn a_bundle_splits_into_one_certificate_per_block() {
    let bundle = format!("# a comment\n{SELF_SIGNED_PEM}\nsubject=CN=x\n{SELF_SIGNED_PEM}");
    let got = split_pem(bundle.as_bytes());
    assert_eq!(got.len(), 2, "{got:?}");
    for c in &got {
        assert_eq!(c.matches("BEGIN CERTIFICATE").count(), 1, "{c}");
        assert!(!c.contains("a comment"), "{c}");
        assert!(!c.contains("subject="), "{c}");
    }
}

/// ⭐ The name OpenSSL's hash lookup would find, and nothing else. A
/// certificate file podvm writes must NOT read as one, or the stale sweep
/// would delete a link.
#[test]
fn only_a_hash_name_reads_as_a_hash_link() {
    for yes in ["01419da9.0", "002c0b4f.12", "abcdef01.r0"] {
        assert!(is_hash_link(yes), "{yes}");
    }
    for no in [
        "ca-certificates.crt",
        "podvm-host-ca-000.pem",
        "0141.0",
        "01419da9",
        "01419da9.",
        "zzzzzzzz.0",
        "01419da9.pem",
    ] {
        assert!(!is_hash_link(no), "{no}");
    }
}

/// A rootfs with an announced host bundle and a hash-indexed CApath, and a
/// program to index it with.
fn capath_scratch(name: &str) -> (String, std::path::PathBuf) {
    let d = scratch(name);
    std::fs::create_dir_all(format!("{d}/etc/ssl/certs")).unwrap();
    std::fs::create_dir_all(format!("{d}/usr/bin")).unwrap();
    // ⚠ The image's own root, so the dedup has something to find, plus one
    // name that makes the directory hash-indexed.
    std::fs::write(format!("{d}/etc/ssl/certs/theirs.pem"), SELF_SIGNED_PEM).unwrap();
    std::fs::write(format!("{d}/etc/ssl/certs/01419da9.0"), SELF_SIGNED_PEM).unwrap();
    std::fs::write(format!("{d}/usr/bin/openssl"), b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(
        format!("{d}/usr/bin/openssl"),
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let host = std::env::temp_dir().join(format!("podvm-{name}-{}.pem", std::process::id()));
    std::fs::write(&host, format!("{SELF_SIGNED_PEM}{OTHER_PEM}")).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::set_var("SSL_CERT_FILE", &host) };
    (d, host)
}

/// ⭐ The whole of T-0412's write half: only the roots the image does NOT
/// already carry are written, and a step asks for the one command that can
/// make them findable.
#[test]
fn only_the_roots_the_image_lacks_are_written_and_a_rehash_is_asked_for() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let (d, host) = capath_scratch("cahash");
    let root = Root::open(&d).unwrap();
    let (fs, steps) = ca_hash_dir(&root, &Options::default()).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };

    assert_eq!(fs[0].action, Action::Rewrote, "{:?}", fs[0]);
    assert!(fs[0].degraded, "a trust change is a degradation");
    // ⛔ ONE file: the image already holds the other root, and writing it
    // again is a duplicate `openssl rehash` skips and a file nobody needs.
    let written: Vec<String> = std::fs::read_dir(format!("{d}/etc/ssl/certs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(HOST_CA_FILE_PREFIX))
        .collect();
    assert_eq!(written, vec!["podvm-host-ca-000.pem"], "{written:?}");
    let body = std::fs::read_to_string(format!("{d}/etc/ssl/certs/{}", written[0])).unwrap();
    assert_eq!(body.matches("BEGIN CERTIFICATE").count(), 1, "{body}");
    assert!(body.contains("T-0412"), "{body}");

    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(
        steps[0].argv,
        vec!["/usr/bin/openssl", "rehash", "/etc/ssl/certs"]
    );
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ A second run does not re-ask for the command once the links are
/// there, and it does not delete what the first run wrote. ⚠ The rootfs is
/// content-addressed and SHARED, so this is every container after the first
/// rather than a rerun nobody does.
#[test]
fn a_linked_capath_needs_no_second_rehash_and_keeps_its_files() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let (d, host) = capath_scratch("cahash2");
    let root = Root::open(&d).unwrap();
    let _ = ca_hash_dir(&root, &Options::default()).unwrap();
    // What `openssl rehash` would have made.
    std::os::unix::fs::symlink(
        "podvm-host-ca-000.pem",
        format!("{d}/etc/ssl/certs/deadbeef.0"),
    )
    .unwrap();
    let (fs, steps) = ca_hash_dir(&root, &Options::default()).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };

    assert_eq!(fs[0].action, Action::Unchanged, "{:?}", fs[0]);
    assert!(steps.is_empty(), "{steps:?}");
    assert!(std::path::Path::new(&format!("{d}/etc/ssl/certs/podvm-host-ca-000.pem")).exists());
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ `--no-steps` writes NOTHING. A certificate in a hash directory that
/// nothing has linked is a file no TLS stack reads, so writing it and then
/// refusing to link it would be litter in somebody else's image.
#[test]
fn no_steps_leaves_the_capath_untouched_and_says_what_was_given_up() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let (d, host) = capath_scratch("cahash3");
    let root = Root::open(&d).unwrap();
    let (fs, steps) = ca_hash_dir(
        &root,
        &Options {
            steps: false,
            ..Options::default()
        },
    )
    .unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };

    assert_eq!(fs[0].action, Action::Skipped);
    assert!(fs[0].degraded);
    assert!(fs[0].detail.contains("--no-steps"), "{:?}", fs[0]);
    assert!(steps.is_empty());
    assert!(std::fs::read_dir(format!("{d}/etc/ssl/certs"))
        .unwrap()
        .all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(HOST_CA_FILE_PREFIX)));
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ **A CAfile sitting INSIDE the CApath is not content the CApath
/// holds.** Found on 2026-09-09 against `ghcr.io/void-linux/void-musl`:
/// T-0407 writes the whole announced bundle to
/// `/etc/ssl/certs/ca-certificates.crt`, and counting that made every
/// announced root look present while OpenSSL's hash lookup could still find
/// none of them. Only a `HASH.N` entry, and the file one points at, is
/// reachable by name.
#[test]
fn a_bundle_inside_the_capath_does_not_count_as_indexed() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let (d, host) = capath_scratch("cahash5");
    // ⚠ Exactly what T-0407 leaves behind: every announced root, in one
    // file, in the CApath directory, with no hash link to it.
    std::fs::write(
        format!("{d}/etc/ssl/certs/ca-certificates.crt"),
        format!("{SELF_SIGNED_PEM}{OTHER_PEM}"),
    )
    .unwrap();
    let root = Root::open(&d).unwrap();
    let (fs, steps) = ca_hash_dir(&root, &Options::default()).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };
    assert_eq!(fs[0].action, Action::Rewrote, "{:?}", fs[0]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(
        std::path::Path::new(&format!("{d}/etc/ssl/certs/podvm-host-ca-000.pem")).exists(),
        "the root the index cannot reach was treated as present"
    );
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⛔ A directory that is not hash-indexed is not one podvm invents an
/// index in: podvm writes where the image's TLS stack already looks.
#[test]
fn a_capath_that_is_not_hash_indexed_is_left_alone() {
    let _serialised = ANNOUNCED_BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    let d = scratch("cahash4");
    std::fs::create_dir_all(format!("{d}/etc/ssl/certs")).unwrap();
    std::fs::write(
        format!("{d}/etc/ssl/certs/ca-certificates.crt"),
        SELF_SIGNED_PEM,
    )
    .unwrap();
    let host = std::env::temp_dir().join(format!("podvm-cahash4-{}.pem", std::process::id()));
    std::fs::write(&host, OTHER_PEM).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::set_var("SSL_CERT_FILE", &host) };
    let root = Root::open(&d).unwrap();
    let (fs, steps) = ca_hash_dir(&root, &Options::default()).unwrap();
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("SSL_CERT_FILE") };
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(!fs[0].degraded, "nothing was given up: there is no CApath");
    assert!(steps.is_empty());
    let _ = std::fs::remove_file(&host);
    let _ = std::fs::remove_dir_all(&d);
}

/// ⚠ Any certificate at all; nothing here checks its contents, only that
/// the PEM reader of `pod-tls` finds one, which is what `host_bundle_bytes`
/// requires.
const SELF_SIGNED_PEM: &str = "\
-----BEGIN CERTIFICATE-----
MIIBITCBxaADAgECAgEBMAoGCCqGSM49BAMCMA8xDTALBgNVBAMMBHRlc3QwHhcN
MjQwMTAxMDAwMDAwWhcNMzQwMTAxMDAwMDAwWjAPMQ0wCwYDVQQDDAR0ZXN0MFkw
EwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEwqQ0KJp8yQ4XKm1UGT1qkPD0iCJ0V7gI
Hs9pTfl0RCgW8gJ0KsCTb2xkPz8dK0FtnZL7lS+7CqvBqYc0z0m4hqMdMBswDAYD
VR0TAQH/BAIwADALBgNVHQ8EBAMCBeAwCgYIKoZIzj0EAwIDSAAwRQIhAP0J7d1z
bB0hLYnzMkGqXn6c8Q9d8sZmT7d5m7z1KQvVAiA5cBqAqCkTLh3n3sVh6Nn3F8mR
7dZQeK1p3QAoLtQ9tA==
-----END CERTIFICATE-----
";

/// ⚠ A SECOND certificate, and it has to be a different one: T-0412's write
/// half exists to add the roots an image does not already carry, and a test
/// whose two roots were the same would pass whether that worked or not.
/// Generated on 2026-09-09 with
/// `openssl req -x509 -newkey ec -subj /CN=podvm-test-other`.
const OTHER_PEM: &str = "\
-----BEGIN CERTIFICATE-----
MIIBjjCCATOgAwIBAgIUKdYowWIy0Gmd3YUnjF0hYcawgf0wCgYIKoZIzj0EAwIw
HDEaMBgGA1UEAwwRcG9kYm94LXRlc3Qtb3RoZXIwHhcNMjYwOTA5MTcyNzIyWhcN
MzYwOTA2MTcyNzIyWjAcMRowGAYDVQQDDBFwb2Rib3gtdGVzdC1vdGhlcjBZMBMG
ByqGSM49AgEGCCqGSM49AwEHA0IABFqqhv1zdvRlJb75xIArQwVSQ7Pi++4g2bJS
62kvM4fB90fq2bo1CpjOAXlYgjzlVJeHC9ECta4eixwVkFaSpGejUzBRMB0GA1Ud
DgQWBBQ2ZA6BxBtrckYJg3zZiHf20uhbljAfBgNVHSMEGDAWgBQ2ZA6BxBtrckYJ
g3zZiHf20uhbljAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0kAMEYCIQCD
9AB9GIOuPkGaeew5iCPAGOcB2p4XjCuQoUjogGrGxgIhAOsLORizWLiyWh6/E9Z9
+rP1OdIyI4oIqBMOX8k8x6Az
-----END CERTIFICATE-----
";
