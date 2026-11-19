//! The test kit of podcrates: a dev-dependency of each crate, never a
//! dependency of a release binary.
//!
//! * [`need_loopback!`] and [`need_localhost!`]: a test that needs TCP or
//!   UDP on 127.0.0.1, or the name `localhost`, skips with a reason on a
//!   host that has none.
//! * [`need_userns!`] and [`need_tool!`]: a test that needs a user and a
//!   mount namespace, or a tool of the host, skips with a reason on a host
//!   that has none.
//! * [`alloc`] and [`switches`]: the allocations, and the voluntary
//!   switches of context (a call that blocks), of a scope on one thread.
//! * [`listener`]: a server child of a test on a port of the loopback that
//!   it holds itself, with new ports where another process took one.
//! * `nobody` (Linux only): a test as a caller with no capabilities; root
//!   starts the test binary again as uid 65534.
//! * `elf`: a static ELF made in the test (x86_64 and aarch64), and
//!   the check of a dynamic one (Linux only).
//! * [`schema`] (feature `schema`): the file of a JSON Schema of `schemas/`
//!   against the types, and a validator of JSON Schema 2020-12.
//! * [`screen`] (feature `screen`): the terminal oracle, a model of a VT
//!   terminal that tells what a person sees after a stream of bytes.
//! * [`stand`] (feature `stand`): the stand-ins of the services of other
//!   parties on the loopback: HTTP, raw TCP and UDP, TLS from the caller;
//!   `stand::warp`, the API of WARP.
//! * [`masque`] (feature `masque`): the HTTP/2 part of a stand-in of WARP's
//!   MASQUE front door, over a stream of the caller.
//! * [`github`] (feature `github`): a fake GitHub for the CI of a
//!   consumer (an App, JIT runners, their broker, `workflow_job` webhooks),
//!   and its binary `pod-fake-github`.
//! * [`githome`] (feature `githome`): a fake of githome's source of jobs for
//!   the CI of podrunner, and its binary `pod-fake-githome`.
//!
//! A stand-in of this kit speaks bytes and HTTP only, and takes no type of
//! a pod crate, so that each crate can dev-depend on the kit. The one
//! exception is the feature `githome`: it speaks the source wire of
//! `pod_job::source`, so that the fake and podrunner read one form of the
//! wire. `pod-job` has no pod dependency and does not dev-depend on the kit.

pub mod alloc;
#[cfg(target_os = "linux")]
pub mod elf;
#[cfg(any(feature = "github", feature = "githome"))]
mod fake;
#[cfg(feature = "githome")]
pub mod githome;
#[cfg(feature = "github")]
pub mod github;
pub mod host;
pub mod listener;
#[cfg(feature = "masque")]
pub mod masque;
#[cfg(target_os = "linux")]
pub mod nobody;
#[cfg(feature = "refused")]
pub mod refused;
#[cfg(feature = "schema")]
pub mod schema;
#[cfg(feature = "screen")]
pub mod screen;
#[cfg(feature = "stand")]
pub mod stand;
pub mod switches;
