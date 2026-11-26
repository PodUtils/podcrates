//! The check that a TCP socket binds on the loopback, for the carriers that
//! `available` answers for (`tcp`, `imap`, `smtp`).

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener};

use crate::carrier::{CarrierError, CarrierErrorKind};

/// The code of "address family not supported" of the host.
#[cfg(unix)]
const NO_FAMILY: i32 = libc::EAFNOSUPPORT;
#[cfg(not(unix))]
const NO_FAMILY: i32 = 10047;

/// A TCP socket binds on the loopback: IPv4 first, then IPv6. The error
/// names each family that failed.
pub(crate) fn tcp_available(carrier: &'static str) -> Result<(), CarrierError> {
    let Err(v4) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return Ok(()) };
    let Err(v6) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) else { return Ok(()) };
    Err(CarrierError::new(
        carrier,
        CarrierErrorKind::Unavailable,
        format!("TCP sockets on the loopback: {}", both(&v4, &v6)),
    ))
}

/// The words of two failed binds. A family that the host lacks is no cause:
/// the words say it is absent, so the error of the other family stays.
fn both(v4: &io::Error, v6: &io::Error) -> String {
    format!("127.0.0.1: {}; ::1: {}", side(v4, "IPv4"), side(v6, "IPv6"))
}

fn side(e: &io::Error, family: &str) -> String {
    let absent = matches!(e.kind(), io::ErrorKind::Unsupported | io::ErrorKind::AddrNotAvailable)
        || e.raw_os_error() == Some(NO_FAMILY);
    if absent { format!("no {family} on this host") } else { pod_core::text::io_words(e) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> io::Error {
        io::Error::from(io::ErrorKind::PermissionDenied)
    }

    fn no_family() -> io::Error {
        io::Error::from_raw_os_error(NO_FAMILY)
    }

    #[test]
    fn a_missing_family_never_hides_the_cause_of_the_other_bind() {
        let d = denied().to_string();
        let rows = [
            (denied(), no_family(), format!("127.0.0.1: {d}; ::1: no IPv6 on this host")),
            (no_family(), denied(), format!("127.0.0.1: no IPv4 on this host; ::1: {d}")),
            (denied(), denied(), format!("127.0.0.1: {d}; ::1: {d}")),
        ];
        for (v4, v6, want) in rows {
            assert_eq!(both(&v4, &v6), want);
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_words_of_the_system_show_no_code() {
        let v4 = io::Error::from_raw_os_error(libc::EACCES);
        let line = both(&v4, &no_family());
        assert!(!line.contains("os error"), "{line}");
        assert!(line.starts_with("127.0.0.1: "), "{line}");
        assert!(line.ends_with("; ::1: no IPv6 on this host"), "{line}");
    }

    #[test]
    fn the_loopback_binds_on_this_host_or_says_why() {
        if let Err(e) = tcp_available("test") {
            assert!(e.to_string().contains("TCP sockets on the loopback: 127.0.0.1: "), "{e}");
        }
    }
}
