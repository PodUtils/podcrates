//! The line of a front door that shows another key than the pinned one
//! (PodUtils/podnet#726): the class of the issuer, and each cause once.

use super::*;
use crate::tls::{PIN_MISMATCH, SHOWN, handshake_failure};

const LETS_ENCRYPT: &str = "O=Let's Encrypt, CN=YE1";
const CLOUDFLARE: &str = "O=Cloudflare, Inc., CN=2024-02-27 Self-Signed Root";
const MIDDLEBOX: &str = "O=Anthropic, CN=Egress Gateway SDS Issuing CA (production)";

fn door(last: u8) -> Target {
    Target::Addr(std::net::SocketAddr::from(([162, 159, 198, last], 443)))
}

/// The failure of a door that shows a certificate of `issuer`.
fn shown(door: &Target, issuer: &str) -> Failure {
    let e = std::io::Error::other(format!("unexpected error: {PIN_MISMATCH}; {SHOWN}{issuer}"));
    handshake_failure(&door.to_string(), e)
}

fn refused(door: &Target) -> Failure {
    handshake_failure(&door.to_string(), std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
}

/// Each issuer gives its class and its sentence; a proxy on the way keeps
/// its sentence.
#[test]
fn the_cause_of_another_key_is_named_by_the_issuer_of_the_certificate() {
    let proxy = || Some("http://proxy.test:3128".to_string());
    let table = [
        (LETS_ENCRYPT, Issuer::LetsEncrypt, "WARP's side shows a certificate of Let's Encrypt at times"),
        (CLOUDFLARE, Issuer::Warp, "WARP's side shows such a certificate at times"),
        ("an unnamed issuer", Issuer::Warp, "WARP's side shows such a certificate at times"),
        (MIDDLEBOX, Issuer::Middlebox, "A middlebox on the way terminates TLS; the key is pinned, so no root helps"),
    ];
    for (issuer, class, words) in table {
        assert_eq!(Issuer::of(issuer), class, "{issuer}");
        let failure = shown(&door(2), issuer);
        let why = why_shown(None, &failure);
        assert!(why.starts_with(words), "{issuer}: {why}");
        assert!(!why.contains("proxy"), "a direct road names no proxy: {why}");
        let retry = class != Issuer::Middlebox;
        assert_eq!(why.contains("a later try passes then"), retry, "{issuer}: {why}");
        let why = why_shown(proxy(), &failure);
        assert!(why.contains("the proxy http://proxy.test:3128") || why.contains("The proxy http://proxy.test:3128"));
    }
    assert_eq!(Issuer::of(""), Issuer::Warp, "a failure with no issuer");
}

/// The door read again fails as the first door did: the line says the
/// cause once, with the sentence of its issuer, and the chain keeps the
/// cause.
#[test]
fn a_door_read_again_with_the_first_cause_says_it_once() {
    for issuer in [LETS_ENCRYPT, MIDDLEBOX] {
        let err = shown(&door(2), issuer);
        let failures = vec![(door(2), shown(&door(2), issuer)), (door(1), shown(&door(1), issuer))];
        let failure = after_mismatch(&err, &door(2), failures, |_| None);
        let line = failure.chain();
        assert_eq!(line.matches(PIN_MISMATCH).count(), 1, "{line}");
        assert_eq!(line.matches(issuer).count(), 1, "{line}");
        assert!(is_pin_mismatch(&failure), "{line}");
        assert!(line.contains("does not match either"), "{line}");
        assert!(!failure.text().contains(PIN_MISMATCH), "the text holds no word of its source: {}", failure.text());
    }
}

/// A door read again that fails for another cause is named; the first
/// cause is named when no door shows another key now.
#[test]
fn a_door_read_again_with_another_cause_is_named() {
    let err = shown(&door(2), MIDDLEBOX);
    let failures = vec![(door(2), shown(&door(2), MIDDLEBOX)), (door(1), refused(&door(1)))];
    let line = after_mismatch(&err, &door(2), failures, |_| None).chain();
    assert!(line.contains(&door(1).to_string()) && line.contains("connection refused"), "{line}");
    assert!(line.contains("A middlebox on the way"), "{line}");
    assert_eq!(line.matches(PIN_MISMATCH).count(), 1, "{line}");

    let failures = vec![(door(2), refused(&door(2))), (door(1), refused(&door(1)))];
    let line = after_mismatch(&err, &door(2), failures, |_| None).chain();
    assert_eq!(line.matches(PIN_MISMATCH).count(), 1, "{line}");
    assert!(line.contains(&door(1).to_string()) && line.contains(&door(2).to_string()), "{line}");
    assert!(!line.contains("does not match either"), "{line}");
}
