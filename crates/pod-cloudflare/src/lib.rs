//! The Cloudflare tunnel spoken directly: the edge client over QUIC and
//! HTTP/2, Cap'n Proto RPC, the legs and their race, ingress, and the quick
//! tunnels.

pub mod capture;
pub mod cloudflare;
pub mod config;
pub mod edge_discovery;
pub mod edge_h2;
pub mod fail;
#[cfg(any(test, feature = "fuzzing"))]
pub mod fuzzing;
pub mod host;
pub mod ingress;
pub mod metrics;
pub mod origin;
pub mod proxy;
pub mod quicktunnel;
pub mod race;
pub mod services;
#[cfg(any(test, feature = "standin"))]
pub mod standin;
pub mod text;
pub mod tunnel;
pub mod udp;
pub mod ws;
pub mod yaml;

pub use host::{Host, ServiceDispatch, Settings, Words};

#[cfg(test)]
mod testing;
