//! The options of a socket: an error of a set that is no failure.

use std::io;

/// The result of a set of a socket option (a time limit of a read or a
/// write), with the refusal of a shut socket as `Ok`. Darwin (`sosetopt`
/// of XNU) refuses each socket option with `EINVAL` when both directions
/// of the socket are shut, as after the close of the peer; Linux sets the
/// option. A read of such a socket gives the bytes that it holds, then the
/// end, and a write fails at once: no call waits, so the socket needs no
/// limit. Each other result stays as it is. The `InvalidInput` of the
/// standard library (a time of zero) has no code of the system, so it
/// stays an error.
pub fn shut_needs_no_limit(set: io::Result<()>) -> io::Result<()> {
    shut_on(set, cfg!(target_vendor = "apple"))
}

/// [`shut_needs_no_limit`] on Darwin (`darwin`) or on another system.
fn shut_on(set: io::Result<()>, darwin: bool) -> io::Result<()> {
    // `EINVAL` of Darwin; on Windows the code 22 is another error.
    const EINVAL: i32 = 22;
    match set {
        Err(e) if darwin && e.raw_os_error() == Some(EINVAL) => Ok(()),
        set => set,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn einval() -> io::Result<()> {
        Err(io::Error::from_raw_os_error(22))
    }

    #[test]
    fn only_the_einval_of_darwin_is_a_shut_socket() {
        assert!(shut_on(einval(), true).is_ok(), "the EINVAL of setsockopt on a shut socket of Darwin");
        assert!(shut_on(einval(), false).is_err(), "another system keeps the error");
        assert_eq!(shut_needs_no_limit(einval()).is_ok(), cfg!(target_vendor = "apple"));
        assert!(shut_on(Err(io::Error::from_raw_os_error(9)), true).is_err(), "EBADF stays");
        assert!(shut_on(Ok(()), true).is_ok());
    }

    #[test]
    fn a_time_of_zero_stays_an_error() {
        let zero = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| std::net::TcpStream::connect(l.local_addr()?))
            .and_then(|s| s.set_read_timeout(Some(std::time::Duration::ZERO)));
        let e = shut_on(zero, true).expect_err("a time of zero");
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{e}");
    }
}
