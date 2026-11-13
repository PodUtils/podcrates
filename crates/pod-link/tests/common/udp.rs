//! The helpers of the tests of the udp carrier: a carrier of a made-up
//! identity, a listener on the loopback, and the dial address of a
//! listener.

use pod_identity::{KeyName, PublicKey};
use pod_link::tcp::Keys;
use pod_link::udp::{Udp, UdpListener};

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring, whose listeners take the keys of
/// `clients`.
pub fn carrier(n: u8, clients: &[PublicKey]) -> Udp {
    Udp::new(ring(), id(n)).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener of `udp` on a free port of the loopback.
pub async fn bound(udp: &Udp) -> UdpListener {
    udp.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &UdpListener) -> String {
    format!("{}@{}", key.hex(), door.local_addr())
}
