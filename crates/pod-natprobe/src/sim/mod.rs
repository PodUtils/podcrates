//! Deterministic virtual-time NAT, STUN server, LAN gateway and DNS64 simulator (feature `sim`).
//! Pure `std`, zero threads, zero wall clock, zero OS sockets.

pub mod gateway;
pub mod nat;
pub mod server;
pub mod world;

pub use gateway::{SimGateway, SimPcpMode, SimPmpMode, SimUpnpMode};
pub use nat::{NatLayer, NatSpec, SimAlloc, SimPool};
pub use server::{ServerMode, SimStunServer};
pub use world::{Capture, CapturedPacket, SeededEntropy, SimDns64, SimWorld, drive_sim};
