//! The UDP of the caller for HTTP/3 and for WireGuard: a socket of the
//! system, or the datagrams of another session ([`crate::WarpNet`], for a
//! carried device). A proxy and a relay carry TCP only: a way with no UDP
//! ([`crate::Way::udp`] is `None`) refuses HTTP/3 and WireGuard, so that a
//! session never leaves the host past the way of the caller.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use pod_core::{BoxFuture, Class, Datagrams, Dial, Failure, Stage, SystemDatagrams};

use crate::net::WarpNet;

/// A source of datagrams toward a front door.
pub trait UdpWay: Send + Sync {
    /// The name of the way for a log line.
    fn label(&self) -> String;

    /// Datagrams that reach `door`.
    fn datagrams<'a>(&'a self, door: SocketAddr) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>>;
}

/// A UDP socket of the system of the family of the door, on a port that the
/// system picks.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemUdp;

impl UdpWay for SystemUdp {
    fn label(&self) -> String {
        "direct UDP".into()
    }

    fn datagrams<'a>(&'a self, door: SocketAddr) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>> {
        Box::pin(async move {
            let any: SocketAddr = match door {
                SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
                SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
            };
            let socket = SystemDatagrams::bind(any).await.map_err(|e| {
                Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("a UDP socket toward {door}: {e}"), e)
            })?;
            Ok(Arc::new(socket) as Arc<dyn Datagrams>)
        })
    }
}

impl UdpWay for WarpNet {
    fn label(&self) -> String {
        "the WARP session".into()
    }

    fn datagrams<'a>(&'a self, _door: SocketAddr) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>> {
        Dial::datagrams(self)
    }
}

/// The failure of HTTP/3 or WireGuard on a way that carries no UDP.
pub(crate) fn no_udp(what: &str, label: &str) -> Failure {
    let text = format!("{what} needs UDP to the front door; the way {label} carries TCP only (a proxy or a relay)");
    Failure::new(Stage::Warp, Class::Usage, text)
}
