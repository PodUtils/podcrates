//! The delegated credential: issue, verify, and each named refusal.

use ed25519_dalek::{SigningKey, VerifyingKey};
use pod_cover::{Credential, CredentialError};

const ISSUER: &str = "relay.example";
const NAME: &str = "relay.example";
const NOW: i64 = 1_780_000_000;

fn key(byte: u8) -> SigningKey {
    SigningKey::from_bytes(&[byte; 32])
}

fn key_of(signing: &SigningKey) -> VerifyingKey {
    signing.verifying_key()
}

fn issued(exporter: Option<&[u8]>) -> Credential {
    Credential::issue(&key(7), ISSUER, NAME, NOW - 10, NOW + 600, [0x42; 32], exporter).expect("a credential")
}

#[test]
fn a_credential_verifies_against_its_chain_and_name() {
    let credential = issued(None);
    let chain = key_of(&key(7));
    credential.verify(NAME, NOW, None, &chain).expect("a valid credential");
    // The subject key is the one that the issuer granted.
    assert_eq!(credential.subject_key, [0x42; 32]);
    assert_eq!(credential.issuer, ISSUER);
    assert_eq!(credential.name, NAME);
}

#[test]
fn a_credential_of_another_issuer_is_refused_by_name() {
    let credential = Credential::issue(&key(7), "other.example", NAME, NOW - 10, NOW + 600, [1; 32], None)
        .expect("a credential of another issuer");
    let error = credential.verify(NAME, NOW, None, &key_of(&key(7))).expect_err("another issuer");
    assert_eq!(error, CredentialError::WrongIssuer);
    assert!(error.to_string().contains("issued by another name"), "{error}");
}

#[test]
fn a_credential_for_another_name_is_refused_by_name() {
    let credential = Credential::issue(&key(7), ISSUER, "elsewhere.example", NOW - 10, NOW + 600, [1; 32], None)
        .expect("a credential");
    let error = credential.verify(ISSUER, NOW, None, &key_of(&key(7))).expect_err("another name");
    assert_eq!(error, CredentialError::NameMismatch);
    assert!(error.to_string().contains("for another name"), "{error}");
}

#[test]
fn an_expired_credential_and_a_future_one_are_refused_by_name() {
    let credential = issued(None);
    let chain = key_of(&key(7));
    let expired = credential.verify(NAME, NOW + 601, None, &chain).expect_err("expired");
    assert_eq!(expired, CredentialError::Expired);
    assert!(expired.to_string().contains("expired"), "{expired}");
    let early = credential.verify(NAME, NOW - 11, None, &chain).expect_err("not yet");
    assert_eq!(early, CredentialError::NotYetValid);
    assert!(early.to_string().contains("not valid yet"), "{early}");
    // The ends of the window are inside it.
    credential.verify(NAME, NOW - 10, None, &chain).expect("the first second");
    credential.verify(NAME, NOW + 600, None, &chain).expect("the last second");
}

#[test]
fn a_credential_of_another_exporter_is_refused_by_name() {
    let credential = issued(Some(b"the exporter of the handshake"));
    let chain = key_of(&key(7));
    credential.verify(NAME, NOW, Some(b"the exporter of the handshake"), &chain).expect("the same exporter");
    let error = credential.verify(NAME, NOW, Some(b"another handshake"), &chain).expect_err("another exporter");
    assert_eq!(error, CredentialError::WrongBinding);
    assert!(error.to_string().contains("another exporter"), "{error}");
    // A bound credential with no exporter on the connection is refused.
    let error = credential.verify(NAME, NOW, None, &chain).expect_err("no exporter");
    assert_eq!(error, CredentialError::WrongBinding);
    // An unbound credential with an exporter is refused too.
    let error = issued(None).verify(NAME, NOW, Some(b"x"), &chain).expect_err("an extra exporter");
    assert_eq!(error, CredentialError::WrongBinding);
}

#[test]
fn a_credential_of_another_key_fails_the_signature() {
    let credential = issued(None);
    let error = credential.verify(NAME, NOW, None, &key_of(&key(8))).expect_err("another key");
    assert_eq!(error, CredentialError::Signature);
    // A changed field fails too: the signature covers the whole credential.
    let mut tampered = issued(None);
    tampered.subject_key[0] ^= 0x01;
    let error = tampered.verify(NAME, NOW, None, &key_of(&key(7))).expect_err("a changed subject key");
    assert_eq!(error, CredentialError::Signature);
    let mut renamed = issued(None);
    renamed.not_after += 1;
    let error = renamed.verify(NAME, NOW, None, &key_of(&key(7))).expect_err("a changed window");
    assert_eq!(error, CredentialError::Signature);
}

#[test]
fn an_empty_window_and_a_bad_name_are_refused_at_issue() {
    let error = Credential::issue(&key(7), ISSUER, NAME, NOW, NOW, [1; 32], None).expect_err("an empty window");
    assert_eq!(error, CredentialError::Window);
    let error = Credential::issue(&key(7), "", NAME, NOW, NOW + 10, [1; 32], None).expect_err("no issuer");
    assert_eq!(error, CredentialError::BadIssuer);
    let error = Credential::issue(&key(7), ISSUER, "a name", NOW, NOW + 10, [1; 32], None).expect_err("a space");
    assert_eq!(error, CredentialError::BadName);
}
