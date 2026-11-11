//! Pairing by a short code (podssh T-086): a node's host gives a code that a
//! person reads to the operator, who types it, and the two ends move what a
//! pair's file moved before: the operator's part of the pair and the node's
//! key one way, the operator's key the other. The code is used once and
//! lives [`LIFETIME`]; it never crosses the network.
//!
//! The ends meet at a magic-wormhole mailbox: `pod-wormhole` holds the
//! client, the code, SPAKE2 and the sealed phases, and the caller gives its
//! `pod_wormhole::Config` (its AppID, its name, its way out, its events) and
//! the mailboxes. What the ends say is the exchange's ([`exchange`]), and
//! the runs are [`run`]'s.

use std::time::Duration;

pub mod exchange;
pub mod run;

pub use pod_wormhole::{Code, Failed, mailbox, wormhole};

/// How long a code lives: past it the giver leaves, and the code fails.
pub const LIFETIME: Duration = Duration::from_secs(600);
