//! The helpers of the tests of the h2 carrier: a carrier of a made-up
//! identity, a listener on the loopback, the dial address of a listener,
//! and the stand-in proxy of `common::proxy`.

use std::sync::Arc;

use pod_core::Dial;
use pod_identity::{KeyName, PublicKey};
use pod_link::h2::{H2, H2Listener};
use pod_link::tcp::Keys;

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring, over `way`, whose listeners
/// take the keys of `clients`.
pub fn carrier(n: u8, way: Arc<dyn Dial>, clients: &[PublicKey]) -> H2 {
    H2::new(ring(), id(n), way).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener of `h2` on a free port of the loopback.
pub async fn bound(h2: &H2) -> H2Listener {
    h2.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &H2Listener) -> String {
    format!("{}@https://{}/link", key.hex(), door.local_addr())
}
