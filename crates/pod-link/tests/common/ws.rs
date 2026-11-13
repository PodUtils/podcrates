//! The helpers of the tests of the ws carrier: a carrier of a made-up
//! identity, a listener on the loopback, and the dial address of a
//! listener.

use std::sync::Arc;

use pod_core::Dial;
use pod_identity::{KeyName, PublicKey};
use pod_link::tcp::Keys;
use pod_link::ws::{Ws, WsListener};

use super::tcp::{id, ring};

/// A carrier of the identity `n` on ring, over `way`, whose listeners
/// take the keys of `clients`.
pub fn carrier(n: u8, way: Arc<dyn Dial>, clients: &[PublicKey]) -> Ws {
    Ws::new(ring(), id(n), way).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k))))
}

/// A listener of `ws` on a free port of the loopback.
pub async fn bound(ws: &Ws) -> WsListener {
    ws.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of the listener `door` whose key is `key`.
pub fn at(key: &PublicKey, door: &WsListener) -> String {
    format!("{}@wss://{}/link", key.hex(), door.local_addr())
}
