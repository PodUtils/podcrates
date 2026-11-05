//! The test kit of podcrates: a dev-dependency of each crate, never a
//! dependency of a release binary.
//!
//! * [`need_loopback!`] and [`need_localhost!`]: a test that needs TCP or
//!   UDP on 127.0.0.1, or the name `localhost`, skips with a reason on a
//!   host that has none.
//! * [`screen`] (feature `screen`): the terminal oracle, a model of a VT
//!   terminal that tells what a person sees after a stream of bytes.
//!
//! A stand-in of this kit speaks bytes and HTTP only, and takes no type of
//! a pod crate, so that each crate can dev-depend on the kit.

pub mod host;
#[cfg(feature = "screen")]
pub mod screen;
