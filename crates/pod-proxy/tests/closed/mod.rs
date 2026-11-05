//! A port where nothing listens, held for the whole test: a socket bound to
//! it that does not listen. A connection to it is refused, and no other
//! test can take the port in the meantime, as it could after a listener
//! that was bound and dropped.

use std::net::SocketAddr;

/// The socket that holds the port, and the port's address. Keep the socket
/// until the test ends.
pub fn port() -> (tokio::net::TcpSocket, SocketAddr) {
    let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
    socket.bind("127.0.0.1:0".parse().expect("an address")).expect("bind");
    let addr = socket.local_addr().expect("its address");
    (socket, addr)
}
