//! The crates of the relay's sessions and of its tunnels: pod-session,
//! pod-e2e and pod-wg.

use std::sync::Arc;

/// pod-session: a record of the layer, its secret from X25519, the UDP
/// framing, and a far end's keeper with its sink of lines: the records,
/// the keys and the async keeper link in.
pub fn session() -> String {
    use pod_session::record::{Record, VERSION};
    let mut bytes = Vec::new();
    let encoded = Record::Ping { value: 7 }.encode(&mut bytes).map_or_else(|e| e.to_string(), |()| "a PING".into());
    let mut entropy = pod_session::OsEntropy;
    let exchange = pod_session::ClientHandshake::start(pod_session::Ask::New, pod_session::FEATURES, &mut entropy)
        .map_err(|e| e.to_string())
        .and_then(|(_, open)| {
            let mut out = Vec::new();
            open.encode(&mut out).map(|()| format!("an OPEN of {} bytes", out.len())).map_err(|e| e.to_string())
        });
    let exchange = exchange.unwrap_or_else(|e| e);
    let keeper: pod_session::keep::Keeper<tokio::net::TcpStream> =
        pod_session::keep::Keeper::new(pod_session::resume::RESUME_DEADLINE)
            .with_tell(pod_session::budget::Tell::new(Arc::new(pod_core::NoEvents), "the probe"));
    let census = keeper.census(pod_session::replay::DEFAULT_CAPACITY);
    format!(
        "pod-session: version {VERSION}, {encoded} of {} bytes, {exchange}, a datagram framed in {:?} bytes, {} sessions kept",
        bytes.len(),
        pod_session::udp::framed(b"probe").map(|f| f.len()),
        census.sessions()
    )
}

/// pod-e2e: a transport of the channel from the keys of a handover (a
/// Noise XX handshake between two keys drawn now: snow with its X25519,
/// ChaCha20-Poly1305, SHA-256 and random bytes links in), the bytes of a
/// held channel, a token of one leg, and a node's end with its holds.
pub fn e2e() -> String {
    use pod_e2e::hold::{ChannelState, state};
    let keys = zeroize::Zeroizing::new([5u8; 64]);
    let transport = state::transport_of(&keys).map_or_else(|e| e, |_| "a transport from the keys".into());
    let held = ChannelState {
        keys,
        sent: 1,
        received: 2,
        out_ended: false,
        in_ended: false,
        unread: Vec::new(),
        to_layer: Vec::new(),
        peer: [7; 32],
        net: false,
        lease: None,
    };
    let node = pod_e2e::ends::Node::open_to_all(
        Arc::new(pod_identity::Identity::from_seed(&[9; 32])),
        Arc::new(pod_core::NoEvents),
    );
    format!(
        "pod-e2e: {}, {transport}, a held channel of {} bytes, a token of {} digits, {:?}, a node of {} holds",
        pod_e2e::PATTERN,
        held.encode().len(),
        pod_e2e::legs::token(&[1; 32], 1).len(),
        pod_e2e::Ask::read(Some("probe.example:22")),
        node.holds.len()
    )
}

/// pod-wg: one handshake with the stand-in in memory, a packet each way,
/// and the timers of the tunnel: ChaCha20-Poly1305 of ring, X25519 of
/// curve25519-dalek and the BLAKE2s of the crate link in.
pub fn wg() -> String {
    use pod_wg::standin;
    let now = std::time::Instant::now();
    let (keys, peer_private) = standin::keys([1, 2, 3]);
    let shook = pod_wg::initiate(&keys).map_err(|e| e.to_string()).and_then(|init| {
        let (response, peer_send, peer_recv, _) = standin::respond(&peer_private, &keys.public(), &init.message);
        let mut session = init.finish(&keys, &response, now).map_err(|e| e.to_string())?;
        let mut packet = vec![0x45, 0, 0, 20];
        packet.resize(20, 0);
        let out = session.seal(&keys.reserved, &packet);
        let mut peer = standin::session(peer_send);
        peer.remote = session.local;
        let back = session.open(&peer.seal(&[0; 3], &packet)).map_err(|e| e.to_string())?;
        let ok = back == packet && peer_recv == session.send_key;
        let mut tunnel =
            pod_wg::Tunnel::new(keys.clone(), session, pod_wg::Stamps::new(std::time::SystemTime::now(), now), now);
        let rekey = tunnel.rekey(now + pod_wg::REKEY_AFTER * 2).map_err(|e| e.to_string())?;
        Ok(format!(
            "a handshake, a message of {} bytes out, a packet back: {ok}, a rekey of {:?} bytes",
            out.len(),
            rekey.map(|m| m.len())
        ))
    });
    format!("pod-wg: {}", shook.unwrap_or_else(|e| e))
}
