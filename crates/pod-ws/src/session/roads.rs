//! A session of each road of the relay, on the stream that the road gives:
//! TLS, TCP alone on the cleartext road, or a stream of HTTP/2. Each keeps
//! the address of the TCP connection that carries it.

use std::net::SocketAddr;
use std::time::Duration;

use pod_core::Connected;

use super::{RelaySession, RelayStream};
use crate::url::Stream;

impl RelaySession<Stream> {
    /// A session over the relay's TLS stream, as [`crate::connect`] opens it,
    /// which keeps `peer`, the address of its TCP connection
    /// ([`RelaySession::peer`]). `write_timeout` bounds each write.
    pub fn over_tls(
        stream: RelayStream,
        peer: Option<SocketAddr>,
        pending: Vec<u8>,
        idle: Option<Duration>,
        write_timeout: Duration,
    ) -> Self {
        let mut session = Self::new(Stream::Tls(Box::new(stream)), pending, idle, write_timeout);
        session.peer = peer;
        session
    }

    /// A session over a connection with no TLS, as the cleartext road opens
    /// it ([`crate::cleartext::connect`]), which keeps the address of its
    /// connection too.
    pub fn over_cleartext(
        connected: Connected,
        pending: Vec<u8>,
        idle: Option<Duration>,
        write_timeout: Duration,
    ) -> Self {
        let mut session = Self::new(Stream::Plain(connected.io), pending, idle, write_timeout);
        session.peer = connected.peer;
        session
    }

    /// A session on a stream of HTTP/2 ([`crate::h2_road`]), with the address
    /// of the connection that it may share with others.
    pub fn over_h2(
        stream: crate::h2_road::Stream,
        peer: Option<SocketAddr>,
        idle: Option<Duration>,
        write_timeout: Duration,
    ) -> Self {
        let mut session = Self::new(Stream::H2(Box::new(stream)), Vec::new(), idle, write_timeout);
        session.peer = peer;
        session
    }
}
