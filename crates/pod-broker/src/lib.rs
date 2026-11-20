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
//! * [`control`]: `load` (master tier), `drop` (operate) and `list` (read)
//!   as rows of the catalogue of `pod-control`, protocol 1, on a Unix socket
//!   of mode 0600.
//! * [`audit`]: one JSON line for each connection, with no secret value.
//!
//! `schemas/broker-policy.v1.schema.json` and `schemas/log.v1.schema.json`
//! come from [`Policy`] and [`Line`]: a test writes them and compares them.
//! * [`tunnel`]: the inside of an allowed tunnel; [`TunnelEnd`] is its seam.
//! * [`tls_end`]: the TLS end (T-100): a CA for each job ([`ca`]), the
//!   placeholder credentials of a policy ([`credentials`]) swapped for the
//!   values of their secrets ([`vault`]) inside the TLS of the tunnel, and
//!   each value scrubbed out of the answers ([`inner`], [`replace`]).
//!
//! **A library only** (docs/decisions.md, C27): each product hosts the
//! broker in a thin `main` and gives the settings, the resolver, the way out
//! and the audit sink. The crate reads no variable, prints nothing and
//! writes its diagnostics to `tracing`.

pub mod audit;
mod broker;
pub mod ca;
pub mod control;
pub mod credentials;
pub mod deny;
mod forward;
pub mod inner;
pub mod jobs;
#[cfg(any(test, feature = "schema"))]
mod json_schema;
pub mod names;
pub mod policy;
mod prefixed;
pub mod replace;
#[cfg(test)]
mod schemas;
mod serve;
pub mod target;
mod tls_body;
pub mod tls_end;
pub mod tunnel;
pub mod vault;
mod words;

pub use audit::{Audit, JsonLines, Line, Memory, Tracing};
#[cfg(unix)]
pub use broker::bind_unix;
pub use broker::{Broker, Settings, upstream};
pub use credentials::{Credential, Place, Secrets};
pub use deny::DenyList;
pub use jobs::{JobInfo, Load, Token};
pub use names::UpstreamCheck;
pub use policy::{Compiled, Mode, NoRule, Policy, Rule, Verdict};
pub use tls_end::TlsEnd;
pub use tunnel::{Counts, Report, Splice, Tunnel, TunnelEnd};
pub use vault::Vault;
