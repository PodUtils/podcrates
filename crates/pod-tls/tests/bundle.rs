//! **The CA bundle, and the first plant of the TLS connector.**
//!
//! **A plant that has never failed is indistinguishable from a plant that
//! always passes.** Each of the two below asserts the *specific* failure, not
//! merely that something failed, so a defect that reddened the suite for an
//! unrelated reason cannot pass a plant that only looked at the outcome.

use std::path::{Path, PathBuf};

use pod_tls::bundle;

#[allow(dead_code)]
mod tls {
    include!("common/tls_for_test.rs");
}

/// A real certificate, in PEM, used to check the decoder against known bytes.
/// This is `webpki-roots`' own first entry — a DigiCert root — and the base64
/// below is checked against it in `plant_pem_decoding_is_byte_exact`.
const DIGICERT_ECC_ROOT_G5: &str = "-----BEGIN CERTIFICATE-----
MIICGTCCAZ+gAwIBAgIQCeCTZaz32ci5PhwLBCou8zAKBggqhkjOPQQDAzBOMQsw
CQYDVQQGEwJVUzEXMBUGA1UEChMORGlnaUNlcnQsIEluYy4xJjAkBgNVBAMTHURp
Z2lDZXJ0IFRMUyBFQ0MgUDM4NCBSb290IEc1MB4XDTIxMDExNTAwMDAwMFoXDTQ2
MDExNDIzNTk1OVowTjELMAkGA1UEBhMCVVMxFzAVBgNVBAoTDkRpZ2lDZXJ0LCBJ
bmMuMSYwJAYDVQQDEx1EaWdpQ2VydCBUTFMgRUNDIFAzODQgUm9vdCBHNTB2MBAG
ByqGSM49AgEGBSuBBAAiA2IABMFEoc8Rl1Ca3iOCNQfN0MsYndLxf3c1TzvdlHJS
7cI7+Oz6e2tYIOyZrsn8aLN1udsJ7MgT9U7GCh1mMEy7H0cKPGEQQil8pQgO4CLp
0zVozptjn4S1mU1YoI71VOeVyaNCMEAwHQYDVR0OBBYEFMFRRVBZqz7nLFr6ICIS
B4CIfBFqMA4GA1UdDwEB/wQEAwIBhjAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49
BAMDA2gAMGUCMQCJao1H5+z8blUD2WdsJk6Dxv3J+ysTvLd6jLRl0mlpYxNjOyZQ
LgGheQaRnUi/wr4CMEfDFXuxoJGZSZOoPHzoRgaLLPIxAJSdYsiJvRmEFOml+wG4
DXZDjC5Ty3zfDBeWUA==
-----END CERTIFICATE-----
";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── the PEM decoder ─────────────────────────────────────────────────────────

/// **Byte-exact.** The first bytes of the decoded certificate are asserted
/// as hex, so a decoder that is off by one base64 character — the failure that
/// produces a certificate that parses as garbage and a chain error that reads
/// like an untrusted issuer — cannot pass.
#[test]
fn plant_pem_decoding_is_byte_exact() {
    let certs = bundle::pem_certificates(DIGICERT_ECC_ROOT_G5.as_bytes()).expect("a single PEM certificate");
    assert_eq!(certs.len(), 1);
    // A DER SEQUENCE, tag 0x30. The whole length follows; the point is that
    // the first byte is right, which a shifted decoder gets wrong.
    assert_eq!(certs[0].as_ref()[0], 0x30);
    // **The exact length and the exact first eight bytes**, measured from
    // the same PEM by decoding it independently. A decoder off by one base64
    // character produces a certificate of the wrong length that still starts
    // 0x30, so the length is the half that catches it.
    assert_eq!(certs[0].as_ref().len(), 541, "the certificate was mis-decoded");
    assert_eq!(hex(&certs[0].as_ref()[..8]), "308202193082019f");
}

#[test]
fn a_bundle_with_several_certificates_yields_all_of_them() {
    let two = format!("{DIGICERT_ECC_ROOT_G5}\n{DIGICERT_ECC_ROOT_G5}");
    let certs = bundle::pem_certificates(two.as_bytes()).expect("two certificates");
    assert_eq!(certs.len(), 2);
    assert_eq!(certs[0], certs[1], "the same input parsed two ways");
}

/// **Comments and blank lines between blocks are ignored.** A real CA
/// bundle carries human-readable text, and a decoder that refused it would
/// reject bundles that work everywhere else.
#[test]
fn text_between_blocks_is_ignored() {
    let with_comments = format!(
        "# a bundle installed by an operator\n\n{DIGICERT_ECC_ROOT_G5}\n\
         # and a note between two certificates\n{DIGICERT_ECC_ROOT_G5}\n"
    );
    let certs = bundle::pem_certificates(with_comments.as_bytes()).expect("comments ignored");
    assert_eq!(certs.len(), 2);
}

/// **A `PRIVATE KEY` block is not a certificate.** A trust bundle carrying
/// one holds a key in a world-readable file, and a parser that skipped it
/// silently would hide that.
#[test]
fn a_private_key_block_is_not_read_as_a_certificate() {
    let pem =
        "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg\n-----END PRIVATE KEY-----\n";
    let certs = bundle::pem_certificates(pem.as_bytes()).expect("an empty list is not an error");
    assert!(certs.is_empty(), "a private key was read as a certificate");
}

#[test]
fn a_begin_without_an_end_is_an_error() {
    let pem = "-----BEGIN CERTIFICATE-----\nMIIB\n";
    let err = bundle::pem_certificates(pem.as_bytes()).expect_err("an unterminated block");
    assert!(err.contains("END"), "got {err}");
}

/// A block with no bytes is no certificate: refused here, by its name, and
/// not as a parse error of the trust store later (the fuzz target
/// `pem_bundle` reads this function).
#[test]
fn an_empty_block_is_an_error() {
    for pem in [
        "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n",
        "-----BEGIN CERTIFICATE----- \r\n \n-----END CERTIFICATE-----",
    ] {
        let err = bundle::pem_certificates(pem.as_bytes()).expect_err("an empty block");
        assert!(err.contains("empty"), "{err}");
    }
}

#[test]
fn non_base64_inside_a_block_is_an_error() {
    let pem = "-----BEGIN CERTIFICATE-----\nnot base64 at all!\n-----END CERTIFICATE-----\n";
    assert!(bundle::pem_certificates(pem.as_bytes()).is_err());
}

// ── PLANT 1: a CA bundle path that does not exist ───────────────────────────

/// **The first plant: a CA bundle path that does not exist must `FAIL`,
/// with the path it tried.**
#[test]
fn plant_a_missing_ca_bundle_fails_and_names_the_path() {
    let missing = PathBuf::from("/nonexistent/podssh-does-not-exist-ca.pem");
    let err = pod_tls::roots_from_bundle(&missing).expect_err("a missing bundle");

    match err {
        pod_tls::Error::Bundle { path, why } => {
            // **The path must be in the message.** A deployment that cannot
            // find its trust store cannot be diagnosed from "TLS failed", and
            // naming the path is the whole point of this plant.
            assert_eq!(path, missing.display().to_string());
            assert!(
                why.to_lowercase().contains("no such file") || why.to_lowercase().contains("cannot find"),
                "the reason must name the OS error, got: {why}"
            );
        }
        other => panic!("expected a Bundle error, got {other:?}"),
    }
}

// ── a bundle that parses but holds nothing usable ───────────────────────────

/// **A PEM file with no CERTIFICATE block is an error, not an empty trust
/// store.** An empty `RootCertStore` rejects every chain with a message that
/// reads like a network fault, so the two must not be confusable.
#[test]
fn a_pem_file_with_no_certificate_is_an_error() {
    let path = Path::new("/tmp/podssh-empty-bundle.pem");
    let err = bundle::parse_bundle(path, b"# nothing but a comment\n").expect_err("a bundle with no certificate");
    assert!(err.contains("no CERTIFICATE block"), "the message must say what was wrong, got: {err}");
}

/// **A file that is not UTF-8 is an error.** A binary trust store is a
/// deployment mistake, and "not a PEM bundle" is a far better report than a
/// chain error forty seconds later.
#[test]
fn a_binary_file_is_not_a_bundle() {
    let err = bundle::parse_bundle(Path::new("/tmp/x.pem"), &[0xff, 0xfe, 0x00, 0x01]).expect_err("binary is not PEM");
    assert!(err.contains("UTF-8"), "got {err}");
}

// ── the default trust store ─────────────────────────────────────────────────

/// With no explicit file, a context still has trust anchors: the compiled-in
/// set is always there, so a host with no CA bundle can connect.
#[test]
fn the_default_trust_store_is_never_empty() {
    let context = tls::context(tls::providers()[0].1.clone());
    let roots = context.roots_for(&pod_tls::Trust::Default).expect("default roots");
    assert!(roots.count >= 100, "only {} anchors", roots.count);
    assert!(roots.source.contains("compiled-in"), "{}", roots.source);
}

/// An explicit file is used alone: a missing one is an error naming the path,
/// never a silent fall back to the defaults.
#[test]
fn an_explicit_trust_file_is_never_widened() {
    let missing = PathBuf::from("/nonexistent/explicit-ca.pem");
    let context = tls::context(tls::providers()[0].1.clone());
    let err = context.roots_for(&pod_tls::Trust::File(missing)).unwrap_err();
    assert!(err.to_string().contains("explicit-ca.pem"), "{err}");
}

/// The extra bundle of the settings (podssh's `podssh-ca.pem` next to its
/// binary) adds to the default roots, and names its file; a missing one is
/// passed over.
#[test]
fn an_extra_bundle_adds_to_the_default_roots_and_a_missing_one_is_passed_over() {
    let dir = std::env::temp_dir().join(format!("pod-tls-extra-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("podssh-ca.pem");
    std::fs::write(&path, DIGICERT_ECC_ROOT_G5).expect("the bundle is written");
    let provider = tls::providers()[0].1.clone();
    let mut settings = pod_tls::Settings::new(tls::podssh_words());
    settings.system_bundles.clear();
    let plain = pod_tls::Context::new(provider.clone(), settings.clone()).default_roots();
    settings.extra_bundle = Some(path.clone());
    let with = pod_tls::Context::new(provider.clone(), settings.clone()).default_roots();
    settings.extra_bundle = Some(dir.join("absent.pem"));
    let missing = pod_tls::Context::new(provider, settings).default_roots();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(with.count, plain.count + 1);
    assert!(with.source.contains(&format!("1 from {}", path.display())), "{}", with.source);
    assert_eq!(missing.count, plain.count);
    assert_eq!(missing.source, plain.source);
}
