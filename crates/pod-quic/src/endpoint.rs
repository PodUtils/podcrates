//! The endpoints: on a UDP socket of the system, or on any
//! [`Datagrams`] (WARP, UDP in a stream of the relay). Each has a reset key
//! of its own ([`ResetKey`]), and a server has a token key of its own
//! ([`TokenKey`]). Each function must run in a tokio runtime, which drives
//! the endpoint.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;

use noq::{Endpoint, EndpointConfig, Runtime, ServerConfig, TokioRuntime};
use pod_core::{Datagrams, Failure, Stage};

use crate::bounded::Bounded;
use crate::keys::{ResetKey, TokenKey};
use crate::socket::DatagramSocket;

/// The configuration of an endpoint, with a reset key of its own.
pub fn config() -> Result<EndpointConfig, Failure> {
    Ok(EndpointConfig::new(Arc::new(ResetKey::random()?)))
}

/// The configuration of a server on `crypto` (the TLS of the caller, on its
/// provider), with a token key of its own and noq's default transport.
pub fn server_config(crypto: Arc<dyn noq::crypto::ServerConfig>) -> Result<ServerConfig, Failure> {
    Ok(ServerConfig::new(crypto, Arc::new(TokenKey::random()?)))
}

fn no_socket(e: std::io::Error) -> Failure {
    Failure::io(Stage::Connect, "no UDP socket for QUIC", e)
}

fn no_endpoint(e: std::io::Error) -> Failure {
    Failure::io(Stage::Quic, "no QUIC endpoint", e)
}

/// An endpoint on `socket`, a UDP socket that the caller bound, and a server
/// on it when `server` is given. Its reads have the bound of [`Bounded`].
pub fn on_socket(socket: UdpSocket, server: Option<ServerConfig>) -> Result<Endpoint, Failure> {
    let socket = TokioRuntime.wrap_udp_socket(socket).map_err(no_endpoint)?;
    let socket = Box::new(Bounded::new(socket));
    Endpoint::new_with_abstract_socket(config()?, server, socket, Arc::new(TokioRuntime)).map_err(no_endpoint)
}

/// A client endpoint on a UDP socket bound to `addr` (`[::]:0` or
/// `0.0.0.0:0` for a port that the system picks). A socket of IPv6 also
/// reaches IPv4 where the system allows it.
pub fn bind(addr: SocketAddr) -> Result<Endpoint, Failure> {
    on_socket(dual_stack(addr).map_err(no_socket)?, None)
}

/// A UDP socket bound to `addr`. Windows makes a socket of IPv6 IPv6 only,
/// so an endpoint on `[::]` sent nothing to an address of IPv4; the socket
/// clears `IPV6_V6ONLY` before its bind, as `noq::Endpoint::client` does.
pub(crate) fn dual_stack(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
    if addr.is_ipv6() {
        // A system with no dual-stack socket keeps IPv6 only.
        let _ = socket.set_only_v6(false);
    }
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

/// An endpoint for one connection: a UDP socket of the family `v6`, on a
/// port that the system picks. A socket that does not bind is the probe's
/// answer: no QUIC here.
pub fn system(v6: bool) -> Result<Endpoint, Failure> {
    match v6 {
        true => bind((Ipv6Addr::UNSPECIFIED, 0).into()),
        false => bind((Ipv4Addr::UNSPECIFIED, 0).into()),
    }
}

/// A server endpoint on a UDP socket bound to `addr`.
pub fn server(config: ServerConfig, addr: SocketAddr) -> Result<Endpoint, Failure> {
    on_socket(UdpSocket::bind(addr).map_err(no_socket)?, Some(config))
}

/// An endpoint whose only socket is `datagrams`, and a server on it when
/// `server` is given. The endpoint dials the real address of the peer; the
/// way carries each datagram there. Its reads have the bound of [`Bounded`].
pub fn over(datagrams: Arc<dyn Datagrams>, server: Option<ServerConfig>) -> Result<Endpoint, Failure> {
    over_with(config()?, datagrams, server)
}

/// [`over`] with the configuration `config` of the caller: the reset key and
/// the generator of the local connection IDs are its own (a rotation
/// schedule is [`crate::migrate::endpoint_config`]).
pub fn over_with(
    config: EndpointConfig,
    datagrams: Arc<dyn Datagrams>,
    server: Option<ServerConfig>,
) -> Result<Endpoint, Failure> {
    let socket = Box::new(Bounded::new(Box::new(DatagramSocket::new(datagrams))));
    Endpoint::new_with_abstract_socket(config, server, socket, Arc::new(TokioRuntime)).map_err(no_endpoint)
}
