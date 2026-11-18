//! An address of the loopback where each TCP connect is refused, and that
//! no other test can take while the caller holds it. A port that a test
//! binds, reads and lets go is free again: a test that runs beside it can
//! bind it, and a connect then reaches that test's server, not a refusal
//! (PodUtils/podnet#653). Here the socket stays bound and does not listen:
//! the kernel refuses each connect, and no other socket can bind the port.

use std::net::{Ipv4Addr, SocketAddr};

/// Holds the port of [`refused`]: while it lives, a connect to the address
/// is refused. A drop lets the port go.
#[derive(Debug)]
pub struct Refused {
    _socket: tokio::net::TcpSocket,
}

/// A bound socket that does not listen on `127.0.0.1`, and its address.
/// A connect to the address is refused while the guard lives. Needs no
/// runtime of tokio.
pub fn refused() -> (Refused, SocketAddr) {
    let socket = tokio::net::TcpSocket::new_v4().expect("a TCP socket");
    socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("a port of the loopback");
    let at = socket.local_addr().expect("its address");
    (Refused { _socket: socket }, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    /// Each connect is refused while the guard lives, also with other
    /// sockets bound to port 0 beside it; no bind takes the port.
    #[test]
    fn a_refused_address_stays_refused_while_its_guard_lives() {
        let (guard, at) = refused();
        let others: Vec<TcpListener> = (0..64).map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()).collect();
        assert!(others.iter().all(|l| l.local_addr().unwrap() != at), "a bind to port 0 took the held port");
        assert!(TcpListener::bind(at).is_err(), "a bind of the held port passed");
        for _ in 0..8 {
            let e = TcpStream::connect_timeout(&at, Duration::from_secs(5)).expect_err("a connect passed");
            assert_eq!(e.kind(), ErrorKind::ConnectionRefused, "{e}");
        }
        drop(guard);
        let free = TcpListener::bind(at).expect("the port is free after the drop");
        drop(free);
    }
}
