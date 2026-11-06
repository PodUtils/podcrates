//! A port where nothing listens, held for the whole test: a socket bound to
//! it that does not listen. A connection to it is refused, and no other
//! test can take the port in the meantime, as it could after a listener
//! that was bound and dropped.

use std::net::SocketAddr;

/// The socket that holds the port, and the port's address. Keep the socket
/// until the test ends.
///
/// macOS drops a SYN to a port that a socket holds and does not listen on,
/// so a dial there times out and is not refused. There the port is free
/// (a listener bound and dropped), and another test can take it first.
pub fn port() -> (Option<tokio::net::TcpSocket>, SocketAddr) {
    if cfg!(target_os = "macos") {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        return (None, listener.local_addr().expect("its address"));
    }
    let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
    socket.bind("127.0.0.1:0".parse().expect("an address")).expect("bind");
    let addr = socket.local_addr().expect("its address");
    (Some(socket), addr)
}
