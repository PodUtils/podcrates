//! The policy of the consumer: the stack calls these hooks, and the
//! consumer gives them in [`crate::StackConfig`].

use std::net::SocketAddr;

/// The admit of a target. The stack asks it before each listen, each
/// connect, each UDP bind and each UDP send. With no `Admit`, the stack
/// admits each target in its subnet.
///
/// A closure `Fn(SocketAddr) -> bool` is an `Admit`.
pub trait Admit: Send + Sync {
    /// True when the stack may bind or reach `target`.
    fn admits(&self, target: SocketAddr) -> bool;
}

impl<F: Fn(SocketAddr) -> bool + Send + Sync> Admit for F {
    fn admits(&self, target: SocketAddr) -> bool {
        self(target)
    }
}

/// A UDP service at the IPv4 gateway of the subnet (for example a resolver
/// at port 53). The stack owns the gateway address, binds [`port`] on it and
/// gives each datagram to [`answer`]. The service sees a datagram of at most
/// [`GATEWAY_DATAGRAM`] bytes; the stack drops a longer one.
///
/// [`port`]: GatewayService::port
/// [`answer`]: GatewayService::answer
pub trait GatewayService: Send + Sync {
    /// The UDP port of the service at the gateway.
    fn port(&self) -> u16;

    /// The answer to the datagram `request` from `from`, or `None` for no
    /// answer. It runs in the task of the stack: it must not block.
    fn answer(&self, from: SocketAddr, request: &[u8]) -> Option<Vec<u8>>;
}

/// The largest datagram that a [`GatewayService`] gets: the EDNS size of a
/// DNS message that crosses a path with no fragmentation.
pub const GATEWAY_DATAGRAM: usize = 1232;
