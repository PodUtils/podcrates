use super::*;
use crate::testing::{Scratch, no_in_root};
use crate::{HostBundle, Names};

/// The settings with a bundle of the host that `SSL_CERT_FILE` named.
fn announced(bytes: &str) -> Settings {
    Settings {
        host_bundle: Some(HostBundle {
            path: "/host/ca.pem".into(),
            announced_by: Some("SSL_CERT_FILE".into()),
            bytes: bytes.as_bytes().to_vec(),
        }),
        ..Settings::default()
    }
}

/// The first word, not a part of a word. A comment line is not counted or
/// commented two times.
#[test]
fn comment_out_matches_the_key_and_not_a_substring() {
    let src = "[options]\n#DownloadUser = alpm\nDownloadUser = alpm\nDownloadUserAgent = x\n  DownloadUser=alpm\n";
    let comment = Names::default().comment;
    let (got, hits) = comment_out(src, "DownloadUser", &comment);
    assert_eq!(hits, 2, "{got}");
    assert!(got.contains(&format!("{comment}DownloadUser = alpm\n")), "{got}");
    assert!(got.contains("DownloadUserAgent = x\n"), "{got}");
    assert!(!got.contains(&format!("{comment}#DownloadUser")), "{got}");
}

#[test]
fn pacman_conf_absent_is_skipped_rather_than_failed() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-nopacman");
    let (fs, steps) = pacman(&d.root(), &Settings::default()).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(!fs[0].degraded);
    // No step: a root with no pacman has no keyring to make.
    assert!(steps.is_empty(), "{steps:?}");
}

#[test]
fn the_apt_drop_in_is_idempotent() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-apt");
    std::fs::create_dir_all(d.at("etc/apt")).unwrap();
    let root = d.root();
    let settings = Settings::default();
    assert_eq!(apt(&root, &settings).unwrap()[0].action, Action::Created);
    assert_eq!(apt(&root, &settings).unwrap()[0].action, Action::Unchanged);
    let got = std::fs::read_to_string(d.at(&settings.names.apt_drop_in)).unwrap();
    assert!(got.contains("APT::Sandbox::User \"root\";"), "{got}");
}

/// A root with no CA bundle gets one, or the row says why not; the check of
/// the certificates is never turned off.
#[test]
fn a_rootfs_with_no_ca_bundle_gets_one_or_is_told_why_not() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-ca");
    let root = d.root();
    let fs = ca_bundle(&root, &Settings::default()).unwrap();
    assert_eq!(fs[0].action, Action::Skipped, "{:?}", fs[0]);
    assert!(fs[0].degraded);
    let fs = ca_bundle(&root, &announced(SELF_SIGNED_PEM)).unwrap();
    assert_eq!(fs[0].action, Action::Created, "{:?}", fs[0]);
    assert_eq!(std::fs::read_to_string(d.at(BUNDLE_PATHS[0])).unwrap(), SELF_SIGNED_PEM);
}

/// The append is idempotent and KEEPS the roots of the image: a second run
/// does not grow the file.
#[test]
fn an_announced_host_bundle_is_appended_once_and_the_image_roots_survive() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-caappend");
    std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
    std::fs::write(d.at("etc/ssl/certs/ca-certificates.crt"), "# THE IMAGE OWN ROOTS\n").unwrap();
    let root = d.root();
    let settings = announced(SELF_SIGNED_PEM);
    let first = ca_bundle(&root, &settings).unwrap();
    let second = ca_bundle(&root, &settings).unwrap();
    let got = std::fs::read_to_string(d.at("etc/ssl/certs/ca-certificates.crt")).unwrap();
    assert_eq!(first[0].action, Action::Rewrote, "{:?}", first[0]);
    assert_eq!(second[0].action, Action::Unchanged, "{:?}", second[0]);
    assert!(got.contains("# THE IMAGE OWN ROOTS"), "the image roots were dropped");
    assert_eq!(got.matches(&settings.names.host_ca_marker).count(), 1, "appended twice");
    assert!(first[0].degraded, "a trust change is a degradation");
}

/// With the host CA fixup off, the trust store of the image stays, and the
/// row says what the caller gave up.
#[test]
fn no_host_cas_leaves_the_image_trust_store_byte_identical() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-canohost");
    std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
    let p = d.at("etc/ssl/certs/ca-certificates.crt");
    std::fs::write(&p, "# THE IMAGE OWN ROOTS\n").unwrap();
    let settings = Settings { host_cas: false, ..announced(SELF_SIGNED_PEM) };
    let fs = ca_bundle(&d.root(), &settings).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(fs[0].degraded);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "# THE IMAGE OWN ROOTS\n");
}

/// With the host CA fixup off, an image with no bundle gets no file of the
/// host, named or not: the bundle of the host can hold an intercepting CA.
#[test]
fn no_host_cas_writes_no_host_bundle_into_an_image_with_none() {
    if no_in_root() {
        return;
    }
    let mut unnamed = announced(SELF_SIGNED_PEM);
    if let Some(h) = unnamed.host_bundle.as_mut() {
        h.announced_by = None;
    }
    for (name, given) in [("pkg-canonenamed", announced(SELF_SIGNED_PEM)), ("pkg-canoneplain", unnamed)] {
        let d = Scratch::new(name);
        std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
        std::fs::create_dir_all(d.at("etc/pki/tls/certs")).unwrap();
        let settings = Settings { host_cas: false, ..given };
        let fs = ca_bundle(&d.root(), &settings).unwrap();
        for p in BUNDLE_PATHS {
            assert!(!d.at(p).exists(), "the host bundle was written into the image at {p} with host_cas off");
        }
        assert_eq!(fs.len(), 1, "{fs:?}");
        assert_eq!(fs[0].action, Action::Skipped, "{:?}", fs[0]);
        assert!(fs[0].degraded, "{:?}", fs[0]);
    }
}

/// A byte of the bundle of the image that is not UTF-8 stays. podvm's code
/// read the bundle as lossy text and wrote each such byte as U+FFFD.
#[test]
fn the_bytes_of_the_image_bundle_survive_the_append() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-cabytes");
    std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
    let theirs = b"# Autorit\xe9 de certification (latin-1)\n".to_vec();
    std::fs::write(d.at("etc/ssl/certs/ca-certificates.crt"), &theirs).unwrap();
    ca_bundle(&d.root(), &announced(SELF_SIGNED_PEM)).unwrap();
    let got = std::fs::read(d.at("etc/ssl/certs/ca-certificates.crt")).unwrap();
    assert!(got.starts_with(&theirs), "{:?}", String::from_utf8_lossy(&got));
}

/// One certificate for each file, as `openssl rehash` needs: with three in a
/// file it links nothing in the directory.
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

/// A name that the hash lookup of OpenSSL finds, and nothing else. A
/// certificate file of the crate must not read as one, or the sweep of old
/// files removes a link.
#[test]
fn only_a_hash_name_reads_as_a_hash_link() {
    for yes in ["01419da9.0", "002c0b4f.12", "abcdef01.r0"] {
        assert!(is_hash_link(yes), "{yes}");
    }
    for no in [
        "ca-certificates.crt",
        "pod-rootfs-host-ca-000.pem",
        "0141.0",
        "01419da9",
        "01419da9.",
        "zzzzzzzz.0",
        "01419da9.pem",
    ] {
        assert!(!is_hash_link(no), "{no}");
    }
}

/// A root with a hash-indexed CA directory, a root of the image in it, and
/// a program to index it.
fn capath_scratch(name: &str) -> Scratch {
    use std::os::unix::fs::PermissionsExt;
    let d = Scratch::new(name);
    std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
    std::fs::create_dir_all(d.at("usr/bin")).unwrap();
    std::fs::write(d.at("etc/ssl/certs/theirs.pem"), SELF_SIGNED_PEM).unwrap();
    std::fs::write(d.at("etc/ssl/certs/01419da9.0"), SELF_SIGNED_PEM).unwrap();
    std::fs::write(d.at("usr/bin/openssl"), b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(d.at("usr/bin/openssl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    d
}

fn ours(d: &Scratch) -> Vec<String> {
    let prefix = Names::default().host_ca_prefix;
    let mut v: Vec<String> = std::fs::read_dir(d.at("etc/ssl/certs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(&prefix))
        .collect();
    v.sort();
    v
}

/// Only the roots that the image does NOT hold are written, and a step asks
/// for the one command that makes them found.
#[test]
fn only_the_roots_the_image_lacks_are_written_and_a_rehash_is_asked_for() {
    if no_in_root() {
        return;
    }
    let d = capath_scratch("pkg-cahash");
    let (fs, steps) = ca_hash_dir(&d.root(), &announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}"))).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote, "{:?}", fs[0]);
    assert!(fs[0].degraded, "a trust change is a degradation");
    // ONE file: the image holds the other root.
    let written = ours(&d);
    assert_eq!(written, vec!["pod-rootfs-host-ca-000.pem"], "{written:?}");
    let body = std::fs::read_to_string(d.at(&format!("etc/ssl/certs/{}", written[0]))).unwrap();
    assert_eq!(body.matches("BEGIN CERTIFICATE").count(), 1, "{body}");
    assert!(body.contains("SSL_CERT_FILE"), "{body}");
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].argv, vec!["/usr/bin/openssl", "rehash", "/etc/ssl/certs"]);
}

/// A second run does not ask for the command again once the links are
/// there, and it keeps what the first run wrote: each container after the
/// first of a shared root.
#[test]
fn a_linked_capath_needs_no_second_rehash_and_keeps_its_files() {
    if no_in_root() {
        return;
    }
    let d = capath_scratch("pkg-cahash2");
    let root = d.root();
    let settings = announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}"));
    let _ = ca_hash_dir(&root, &settings).unwrap();
    // What `openssl rehash` makes.
    std::os::unix::fs::symlink("pod-rootfs-host-ca-000.pem", d.at("etc/ssl/certs/deadbeef.0")).unwrap();
    let (fs, steps) = ca_hash_dir(&root, &settings).unwrap();
    assert_eq!(fs[0].action, Action::Unchanged, "{:?}", fs[0]);
    assert!(steps.is_empty(), "{steps:?}");
    assert!(d.at("etc/ssl/certs/pod-rootfs-host-ca-000.pem").exists());
}

/// A bundle of the host that GREW after the first rehash: the new root gets
/// a file and the step that links it. podvm's code asked for no step when
/// any file of its own had a link, so the new root stayed invisible.
#[test]
fn a_grown_host_bundle_asks_for_a_rehash_again() {
    if no_in_root() {
        return;
    }
    let d = capath_scratch("pkg-cahash6");
    let root = d.root();
    let _ = ca_hash_dir(&root, &announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}"))).unwrap();
    std::os::unix::fs::symlink("pod-rootfs-host-ca-000.pem", d.at("etc/ssl/certs/deadbeef.0")).unwrap();
    let (fs, steps) = ca_hash_dir(&root, &announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}{THIRD_PEM}"))).unwrap();
    assert_eq!(ours(&d), vec!["pod-rootfs-host-ca-000.pem", "pod-rootfs-host-ca-001.pem"]);
    assert_eq!(fs[0].action, Action::Rewrote, "{:?}", fs[0]);
    assert_eq!(steps.len(), 1, "the new root has no link and no step asks for one: {fs:#?}");
}

/// With the steps off the crate writes NOTHING: a certificate with no link
/// in a hash directory is a file that no TLS stack reads.
#[test]
fn no_steps_leaves_the_capath_untouched_and_says_what_was_given_up() {
    if no_in_root() {
        return;
    }
    let d = capath_scratch("pkg-cahash3");
    let settings = Settings { steps: false, ..announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}")) };
    let (fs, steps) = ca_hash_dir(&d.root(), &settings).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(fs[0].degraded);
    assert!(fs[0].detail.contains("steps are off"), "{:?}", fs[0]);
    assert!(steps.is_empty());
    assert!(ours(&d).is_empty());
}

/// A CA file INSIDE the CA directory is not content of the index: OpenSSL
/// finds a certificate by name, so only a `HASH.N` entry and the file that
/// one names count.
#[test]
fn a_bundle_inside_the_capath_does_not_count_as_indexed() {
    if no_in_root() {
        return;
    }
    let d = capath_scratch("pkg-cahash5");
    std::fs::write(d.at("etc/ssl/certs/ca-certificates.crt"), format!("{SELF_SIGNED_PEM}{OTHER_PEM}")).unwrap();
    let (fs, steps) = ca_hash_dir(&d.root(), &announced(&format!("{SELF_SIGNED_PEM}{OTHER_PEM}"))).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote, "{:?}", fs[0]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(
        d.at("etc/ssl/certs/pod-rootfs-host-ca-000.pem").exists(),
        "a root that the index cannot reach was present"
    );
}

/// A directory that is not hash-indexed gets no new index.
#[test]
fn a_capath_that_is_not_hash_indexed_is_left_alone() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("pkg-cahash4");
    std::fs::create_dir_all(d.at("etc/ssl/certs")).unwrap();
    std::fs::write(d.at("etc/ssl/certs/ca-certificates.crt"), SELF_SIGNED_PEM).unwrap();
    let (fs, steps) = ca_hash_dir(&d.root(), &announced(OTHER_PEM)).unwrap();
    assert_eq!(fs[0].action, Action::Skipped);
    assert!(!fs[0].degraded, "nothing was given up: there is no CA directory");
    assert!(steps.is_empty());
}

/// A certificate; nothing here reads its content.
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

/// A SECOND, different certificate: a test with two equal roots passes
/// whether the fixup works or not.
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

/// A THIRD block. The split reads the frame of a block, not its content,
/// so this is no real certificate.
const THIRD_PEM: &str = "\
-----BEGIN CERTIFICATE-----
dGhpcmQgYmxvY2sgb2YgdGhlIHRlc3Q=
-----END CERTIFICATE-----
";
