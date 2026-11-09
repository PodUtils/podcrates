//! The egress broker of a job: an HTTP proxy (CONNECT, and plain requests
//! in absolute form) that holds each job to its policy.
//!
//! * [`policy`]: the policy of a job, version 1, in JSON or TOML: the mode
//!   `allow` (only the targets of its rules: a host or `*.NAME` on whole
//!   labels, a path prefix on whole segments, the ports) or `open` (each
//!   target that no deny holds), and extra denies.
//! * [`deny`]: the fixed denies that no policy lifts (the metadata
//!   services, private, loopback, link-local, CGNAT, unspecified,
//!   multicast and reserved addresses, port 25), the addresses of the host
//!   and the deny file of the host.
//! * [`target`]: the strict reading of a target; an ambiguous one gets 400.
//! * [`Broker`]: the jobs ([`Broker::load`], [`Broker::drop_job`],
//!   [`Broker::list`]), one connection ([`Broker::handle`]) and the
//!   listeners ([`Broker::serve_tcp`], [`Broker::serve_unix`]). Each name is
//!   resolved once, each address is checked, and the broker dials only a
//!   checked address, through the way out that the host gives
//!   ([`upstream`]: direct, or an upstream HTTP or SOCKS5 proxy).
//! * [`control`]: `load`, `drop` and `list` as JSON lines on a Unix socket
//!   of mode 0600.
//! * [`audit`]: one JSON line for each connection, with no secret value.
//! * [`tunnel`]: the inside of an allowed tunnel; [`TunnelEnd`] is the seam
//!   of the TLS end of T-100.
//!
//! **A library only** (docs/decisions.md, C27): each product hosts the
//! broker in a thin `main` and gives the settings, the resolver, the way out
//! and the audit sink. The crate reads no variable, prints nothing and
//! writes its diagnostics to `tracing`.

pub mod audit;
mod broker;
pub mod control;
pub mod deny;
mod forward;
pub mod jobs;
pub mod policy;
mod prefixed;
mod serve;
pub mod target;
pub mod tunnel;

pub use audit::{Audit, JsonLines, Line, Memory, Tracing};
pub use broker::{Broker, Settings, upstream};
pub use deny::DenyList;
pub use jobs::{JobInfo, Load, Token};
pub use policy::{Compiled, Mode, Policy, Rule, Verdict};
pub use tunnel::{Counts, Splice, Tunnel, TunnelEnd};
