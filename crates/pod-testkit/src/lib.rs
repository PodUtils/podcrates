//! The test kit of podcrates: a dev-dependency of each crate, never a
//! dependency of a release binary.
//!
//! * [`need_loopback!`] and [`need_localhost!`]: a test that needs TCP or
//!   UDP on 127.0.0.1, or the name `localhost`, skips with a reason on a
//!   host that has none.
//! * [`need_userns!`] and [`need_tool!`]: a test that needs a user and a
//!   mount namespace, or a tool of the host, skips with a reason on a host
//!   that has none.
//! * [`screen`] (feature `screen`): the terminal oracle, a model of a VT
//!   terminal that tells what a person sees after a stream of bytes.
//! * [`stand`] (feature `stand`): the stand-ins of the services of other
//!   parties on the loopback: HTTP, raw TCP and UDP, TLS from the caller.
//!
//! A stand-in of this kit speaks bytes and HTTP only, and takes no type of
//! a pod crate, so that each crate can dev-depend on the kit.

pub mod host;
#[cfg(feature = "screen")]
pub mod screen;
#[cfg(feature = "stand")]
pub mod stand;
