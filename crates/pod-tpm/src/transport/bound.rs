//! The bounds of a transport of a device or of a socket: the time limit of
//! one command, the size of a response, and the class of each failure of
//! the system.

use std::io;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};

use crate::wire::{HEADER, MAX_RESPONSE};

/// The time limits of a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The longest wait for one command: from the write of the command to
    /// the last byte of its response (and the connect of a socket). Past
    /// it the transport closes its device or its connection and is broken:
    /// each next call fails at once with the class `Unavailable`.
    pub command: Duration,
}

impl Default for Limits {
    /// 30 seconds, the limit of podssh's module: a TPM of hardware can take
    /// seconds for a primary key of `TPM2_CreatePrimary`.
    fn default() -> Limits {
        Limits { command: Duration::from_secs(30) }
    }
}

/// A failure at `Stage::Host`.
pub(super) fn host(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, text)
}

/// The class of an error of the system on the way to a TPM. It is not
/// [`Class::of_io`]: a missing device is no TPM (`Unavailable`), and a
/// refusal of the system is `Permission`, not a refusal of a credential.
pub(super) fn class_of(kind: io::ErrorKind) -> Class {
    use io::ErrorKind as K;
    match kind {
        K::TimedOut | K::WouldBlock => Class::Timeout,
        K::UnexpectedEof | K::InvalidData => Class::Protocol,
        K::PermissionDenied => Class::Permission,
        _ => Class::Unavailable,
    }
}

/// `text: e`, with the class of `e`.
pub(super) fn system(text: String, e: io::Error) -> Failure {
    Failure::caused(Stage::Host, class_of(e.kind()), text, e)
}

/// The failure of a transport that broke on an earlier command.
pub(super) fn broken(name: &str) -> Failure {
    host(Class::Unavailable, format!("{name} broke on an earlier command and is closed: open it again"))
}

/// The failure of a command that passed its time limit.
pub(super) fn late(name: &str, limits: &Limits) -> Failure {
    host(Class::Timeout, format!("{name} did not answer in {} ms; it is closed", limits.command.as_millis()))
}

/// The time that remains until `until`; none is a late answer.
pub(super) fn left(until: Instant, name: &str, limits: &Limits) -> Result<Duration, Failure> {
    let left = until.saturating_duration_since(Instant::now());
    if left.is_zero() { Err(late(name, limits)) } else { Ok(left) }
}

/// The size of a response that a header or a prefix says, checked before a
/// byte more is read: from [`HEADER`] to [`MAX_RESPONSE`].
pub(super) fn response_size(size: u32, name: &str) -> Result<usize, Failure> {
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if (HEADER..=MAX_RESPONSE).contains(&size) {
        return Ok(size);
    }
    let why = format!("{name} says a response of {size} bytes: a response has {HEADER} to {MAX_RESPONSE} bytes");
    Err(host(Class::Protocol, why))
}

/// The size field of a response header (bytes 2 to 5).
pub(super) fn size_field(header: &[u8]) -> Option<u32> {
    match header.get(2..6) {
        Some(&[a, b, c, d]) => Some(u32::from_be_bytes([a, b, c, d])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_of_a_system_error_is_of_the_way_to_a_tpm() {
        assert_eq!(class_of(io::ErrorKind::NotFound), Class::Unavailable);
        assert_eq!(class_of(io::ErrorKind::PermissionDenied), Class::Permission);
        assert_eq!(class_of(io::ErrorKind::TimedOut), Class::Timeout);
        assert_eq!(class_of(io::ErrorKind::WouldBlock), Class::Timeout);
        assert_eq!(class_of(io::ErrorKind::UnexpectedEof), Class::Protocol);
        assert_eq!(class_of(io::ErrorKind::BrokenPipe), Class::Unavailable);
        assert_eq!(class_of(io::ErrorKind::ConnectionRefused), Class::Unavailable);
    }

    #[test]
    fn a_response_size_has_its_bounds() {
        assert_eq!(response_size(10, "t").unwrap(), 10);
        assert_eq!(response_size(MAX_RESPONSE as u32, "t").unwrap(), MAX_RESPONSE);
        for bad in [0, 9, MAX_RESPONSE as u32 + 1, u32::MAX] {
            assert_eq!(response_size(bad, "t").unwrap_err().class, Class::Protocol, "{bad}");
        }
    }
}
