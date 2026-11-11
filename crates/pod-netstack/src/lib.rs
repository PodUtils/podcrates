//! The userspace TCP and UDP stack, on smoltcp 0.14.
//!
//! A [`NetStack`] owns a private IPv4 and IPv6 subnet and runs the TCP and
//! UDP state machines for it in a task of tokio. Its link layer is an
//! in-process packet queue ([`device::LinkDevice`]), so the stack makes no
//! `AF_INET` call of the kernel:
//!
//! * [`stack`]: the stack ([`NetStack::loopback`], [`NetStack::over_link`]),
//!   its settings ([`StackConfig`]), its events and counters, and its
//!   sockets: [`TcpStream`] (`AsyncRead` and `AsyncWrite`), [`TcpListener`],
//!   [`UdpSocket`] and [`UdpDatagrams`] (a [`pod_core::Datagrams`]).
//! * [`device`]: the link device of smoltcp over two bounded queues, its
//!   counters, and [`device::RecordingDevice`], which gives each packet to
//!   a [`device::PacketTap`].
//! * [`Subnet`]: the address plan (`10.66.0.0/24` and `fd00:66::/64` by
//!   default) and its gateway.
//! * [`hooks`]: the policy of the consumer: [`Admit`] (may the stack bind
//!   or reach a target) and [`GatewayService`] (a UDP service at the
//!   gateway, such as a resolver).
//!
//! The crate prints nothing, reads no environment variable and keeps no
//! global state. It re-exports [`smoltcp`], so that a consumer names one
//! version of it.
//!
//! **Build setting.** The stack owns more than the two addresses that
//! smoltcp allows by default (its own, the gateways, the address of each
//! listener). Each binary that links the crate sets
//! `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` in the `[env]` table of its
//! `.cargo/config.toml`: Cargo does not read the configuration of a
//! dependency. A build with a lower value fails here.

pub mod device;
pub mod hooks;
pub mod stack;
mod subnet;

pub use device::{DeviceStats, DeviceStatsSnapshot, Direction, LinkDevice, PacketQueue, PacketTap, RecordingDevice};
pub use hooks::{Admit, GatewayService};
pub use smoltcp;
pub use stack::{
    Clock, NetStack, NetStats, NetStatsSnapshot, SocketCount, StackConfig, StackEvent, TcpListener, TcpStream,
    UdpDatagrams, UdpSocket,
};
pub use subnet::Subnet;

/// The addresses that the stack can own: its own two, the two gateways and
/// the address of each listener off its own address.
pub const MAX_ADDRESSES: usize = smoltcp::config::IFACE_MAX_ADDR_COUNT;

const _: () = assert!(
    MAX_ADDRESSES >= 8,
    "build with SMOLTCP_IFACE_MAX_ADDR_COUNT = \"8\" in the [env] table of .cargo/config.toml (pod-netstack README)"
);
