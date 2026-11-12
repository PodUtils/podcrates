//! STUN and TURN for the crates of podcrates and their binaries:
//!
//! * [`stun`] (feature `stun`): the codec of STUN (RFC 8489): the header,
//!   the attributes, the XOR addresses, ERROR-CODE, MESSAGE-INTEGRITY and
//!   MESSAGE-INTEGRITY-SHA256, FINGERPRINT, the long-term keys of MD5 and
//!   SHA-256, the password algorithms, and the methods and attributes of
//!   TURN. Pure Rust, no I/O, no async, no TLS. `pod_proxy::stun` is this
//!   module.
//! * [`md5`] (feature `stun`): MD5, for the long-term key and for HTTP
//!   Digest in `pod-proxy`.
//! * [`Client`] (feature `turn`): a TURN client (RFC 8656) over UDP or over
//!   TLS on TCP: an allocation, its refresh, permissions, channels, Send and
//!   Data indications, ChannelData. The caller gives the server, the
//!   credentials, the dialer, the TLS context, the clock and the source of
//!   the UDP socket in [`Settings`].
//!
//! **No process state, no output.** The crate reads no variable of the
//! environment, keeps no global, prints nothing, and writes its diagnostics
//! to `tracing`. It holds no service and mints no credential: the server and
//! the credentials are the caller's.

#[cfg(feature = "stun")]
pub mod md5;
#[cfg(feature = "stun")]
pub mod stun;

#[cfg(feature = "turn")]
mod client;

#[cfg(feature = "turn")]
pub use client::{
    Client, Clock, Credentials, Error, Event, Kept, Limits, Opened, Settings, SystemUdp, TokioClock, Transport,
    UdpSource,
};
