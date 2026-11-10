//! The endpoints: on a UDP socket of the system, or on any
//! [`Datagrams`] (WARP, UDP in a stream of the relay). Each has a reset key
//! of its own ([`ResetKey`]), and a server has a token key of its own
//! ([`TokenKey`]). Each function must run in a tokio runtime, which drives
//! the endpoint.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;

use noq::{Endpoint, EndpointConfig, ServerConfig, TokioRuntime};
use pod_core::{Class, Datagrams, Failure, Stage};

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
    Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("no UDP socket for QUIC: {e}"), e)
}

fn no_endpoint(e: std::io::Error) -> Failure {
    Failure::caused(Stage::Quic, Class::of_io(e.kind()), format!("no QUIC endpoint: {e}"), e)
}

/// An endpoint on `socket`, a UDP socket that the caller bound, and a server
/// on it when `server` is given.
pub fn on_socket(socket: UdpSocket, server: Option<ServerConfig>) -> Result<Endpoint, Failure> {
    Endpoint::new(config()?, server, socket, Arc::new(TokioRuntime)).map_err(no_endpoint)
}

/// A client endpoint on a UDP socket bound to `addr` (`[::]:0` or
/// `0.0.0.0:0` for a port that the system picks).
pub fn bind(addr: SocketAddr) -> Result<Endpoint, Failure> {
    on_socket(UdpSocket::bind(addr).map_err(no_socket)?, None)
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
/// way carries each datagram there.
pub fn over(datagrams: Arc<dyn Datagrams>, server: Option<ServerConfig>) -> Result<Endpoint, Failure> {
    let socket = Box::new(DatagramSocket::new(datagrams));
    Endpoint::new_with_abstract_socket(config()?, server, socket, Arc::new(TokioRuntime)).map_err(no_endpoint)
}
