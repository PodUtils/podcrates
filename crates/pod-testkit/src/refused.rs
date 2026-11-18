//! An address of the loopback where each TCP connect is refused, and that
//! no other test can take while the caller holds it. A port that a test
//! binds, reads and lets go is free again: a test that runs beside it can
//! bind it, and a connect then reaches that test's server, not a refusal
//! (PodUtils/podnet#653).
//!
//! On Linux and Windows the socket stays bound and does not listen: the
//! kernel refuses each connect, and no other socket can bind the port. The
//! kernel of macOS drops a SYN to such a socket with no reset, so a connect
//! waits for its timeout (T-936). There the port is one with no socket,
//! below the range of the ports that a bind to port 0 gives (49152 to 65535
//! on macOS), and checked once: the kernel resets each connect, and no test of this
//! repository binds a fixed port of the loopback (`check-loopback.py`).

use std::net::{Ipv4Addr, SocketAddr};

/// Holds the port of [`refused`]: while it lives, a connect to the address
/// is refused. A drop lets the port go.
#[derive(Debug)]
pub struct Refused {
    _socket: Option<tokio::net::TcpSocket>,
}

/// An address on `127.0.0.1` where each connect is refused while the guard
/// lives. Needs no runtime of tokio.
pub fn refused() -> (Refused, SocketAddr) {
    if HOLDS_A_BOUND_SOCKET {
        let socket = tokio::net::TcpSocket::new_v4().expect("a TCP socket");
        socket.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("a port of the loopback");
        let at = socket.local_addr().expect("its address");
        return (Refused { _socket: Some(socket) }, at);
    }
    (Refused { _socket: None }, unbound_port())
}

/// True where the kernel refuses a connect to a bound socket that does not
/// listen.
const HOLDS_A_BOUND_SOCKET: bool = cfg!(any(target_os = "linux", target_os = "android", windows));

/// The ports that a bind to port 0 never gives on macOS (its range starts
/// at 49152), above the 1024 of the system.
const UNBOUND_PORTS: std::ops::Range<u16> = 20_000..40_000;

/// A port of [`UNBOUND_PORTS`] where a connect is refused at once.
fn unbound_port() -> SocketAddr {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    for attempt in 0u32..64 {
        let span = u64::from(UNBOUND_PORTS.end - UNBOUND_PORTS.start);
        let port = UNBOUND_PORTS.start + (state.hash_one((std::process::id(), attempt)) % span) as u16;
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let answer = std::net::TcpStream::connect_timeout(&at, std::time::Duration::from_secs(1));
        if matches!(&answer, Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused) {
            return at;
        }
    }
    panic!("no port of {UNBOUND_PORTS:?} on 127.0.0.1 refuses a connect")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    /// Each connect is refused while the guard lives, also with other
    /// sockets bound to port 0 beside it; no bind to port 0 takes the port,
    /// and where the guard holds a socket, no bind of the port passes.
    #[test]
    fn a_refused_address_stays_refused_while_its_guard_lives() {
        let (guard, at) = refused();
        let others: Vec<TcpListener> = (0..64).map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()).collect();
        assert!(others.iter().all(|l| l.local_addr().unwrap() != at), "a bind to port 0 took the held port");
        if HOLDS_A_BOUND_SOCKET {
            assert!(TcpListener::bind(at).is_err(), "a bind of the held port passed");
        }
        for _ in 0..8 {
            let e = TcpStream::connect_timeout(&at, Duration::from_secs(5)).expect_err("a connect passed");
            assert_eq!(e.kind(), ErrorKind::ConnectionRefused, "{e}");
        }
        drop(guard);
        let free = TcpListener::bind(at).expect("the port is free after the drop");
        drop(free);
    }

    /// The form of macOS, on each system: a port with no
    /// socket, out of the range of a bind to port 0, refuses at once.
    #[test]
    fn an_unbound_port_refuses_at_once() {
        let at = unbound_port();
        assert!(UNBOUND_PORTS.contains(&at.port()), "{at}");
        let started = std::time::Instant::now();
        let e = TcpStream::connect_timeout(&at, Duration::from_secs(5)).expect_err("a connect passed");
        assert_eq!(e.kind(), ErrorKind::ConnectionRefused, "{e}");
        assert!(started.elapsed() < Duration::from_secs(2), "the refusal took {:?}", started.elapsed());
    }
}
