//! The helpers of the tests of the ICMP carrier: a carrier of a made-up
//! identity, and an ARQ that ends a lost link fast.

use std::time::Duration;

use pod_identity::{KeyName, PublicKey};
use pod_link::arq::ArqSettings;
use pod_link::icmp::Icmp;
use pod_link::tcp::Keys;

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring, whose listeners take the keys of
/// `clients`.
pub fn carrier(n: u8, clients: &[PublicKey]) -> Icmp {
    Icmp::new(ring(), id(n)).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// An ARQ that gives up a few retries, so a silent peer ends a lost link in
/// about one second.
pub fn fast_arq() -> ArqSettings {
    ArqSettings {
        retries: 3,
        rto_initial: Duration::from_millis(100),
        rto_floor: Duration::from_millis(50),
        rto_ceiling: Duration::from_millis(300),
        ..ArqSettings::default()
    }
}
