//! Wire codecs and strict validators for STUN (RFC 8489/5780), NAT-PMP (RFC 6886),
//! PCP (RFC 6887), UPnP IGD (SSDP/SOAP), and NAT64/DNS64 (RFC 7050/6052).

pub mod addr;
pub mod nat64;
pub mod pcp;
pub mod pmp;
pub mod request;
pub mod ssdp;
pub mod validate;

pub use addr::{AddrClass, AddrPolicy, canonical_addr, canonical_ip, classify_ip, is_global, is_tunnel_iface};
pub use request::{Topology, usable_other};
pub use validate::{ValidateOutcome, Validated, validate_response};
