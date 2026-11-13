//! The helpers of the tests of the ws carrier: a carrier of a made-up
//! identity, a listener on the loopback, the dial address of a listener,
//! and the pump of the carrier over a session in memory.

use std::sync::Arc;

use pod_core::Dial;
use pod_identity::{KeyName, PublicKey};
use pod_link::BoxIo;
use pod_link::tcp::Keys;
use pod_link::ws::{Beat, Ws, WsListener};
use pod_ws::RelaySession;
use tokio::io::DuplexStream;

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

/// The stream of a link that the pump of the carrier gives over a session
/// in memory, with `beat`, and the session of its peer, a server's.
pub fn pumped(beat: Beat) -> (BoxIo, Arc<RelaySession<DuplexStream>>) {
    let (near, far) = tokio::io::duplex(4 << 20);
    let near = RelaySession::new(near, Vec::new(), None, super::LIMIT);
    let peer = pod_ws::server::session(far, Vec::new(), None);
    (pod_link::ws::carry(near, beat), Arc::new(peer))
}
