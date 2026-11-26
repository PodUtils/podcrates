use std::io;

use pod_core::failure::joined;

use super::*;

fn no_socket(kind: io::ErrorKind, words: &str) -> Failure {
    Failure::io(Stage::Connect, "no UDP socket for QUIC", io::Error::new(kind, words))
}

/// An address whose endpoint has no UDP socket: the line of the race says
/// the error of the system, not only "no UDP socket for QUIC"
/// (PodUtils/podssh#360, ask 1).
#[test]
fn a_failed_socket_keeps_its_cause_in_the_line_of_the_race() {
    let failure = none_answered(vec![no_socket(io::ErrorKind::PermissionDenied, "the sandbox refused it")]);
    let line = failure.chain();
    assert!(line.contains("no UDP socket for QUIC: the sandbox refused it"), "{line}");
    assert_eq!(joined(&failure), line);
    assert_eq!(failure.io_cause().map(|e| e.kind()), Some(io::ErrorKind::PermissionDenied));
    assert_eq!(failure.class, Class::of_io(io::ErrorKind::PermissionDenied));
}

/// Each address says its cause once, also one that is not the last.
#[test]
fn each_address_says_its_cause_once() {
    let quoted = io::Error::from(io::ErrorKind::ConnectionRefused);
    let words = quoted.to_string();
    let refused = Failure::caused(Stage::Quic, Class::Refused, format!("192.0.2.2:443: QUIC: {words}"), quoted);
    let failure = none_answered(vec![no_socket(io::ErrorKind::AddrNotAvailable, "no address of IPv6"), refused]);
    let line = failure.chain();
    assert!(line.contains("no UDP socket for QUIC: no address of IPv6"), "{line}");
    assert_eq!(line.matches(&words).count(), 1, "{line}");
    assert_eq!(joined(&failure), line);
}

/// The class: a timeout when each address timed out, else the first other.
#[test]
fn the_class_is_the_first_that_is_not_a_timeout() {
    let silent = || Failure::new(Stage::Quic, Class::Timeout, "192.0.2.1:443: no answer over QUIC within 5 s");
    assert_eq!(none_answered(vec![silent(), silent()]).class, Class::Timeout);
    let mixed = none_answered(vec![silent(), no_socket(io::ErrorKind::PermissionDenied, "refused")]);
    assert_eq!(mixed.class, Class::of_io(io::ErrorKind::PermissionDenied));
    let none = none_answered(Vec::new());
    assert_eq!((none.class, none.text()), (Class::Usage, "no address to try over QUIC"));
}
