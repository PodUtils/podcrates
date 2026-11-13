//! pod-link: two ends of a link over the memory carrier carry one stream,
//! the unix carrier checks the host, and the tcp, the ws, the h2, the poll,
//! the quic and the udp carriers dial their own listeners over TLS with the
//! pin of their key, so that the driver of a link, the frame codec and each
//! carrier link in.

use std::sync::Arc;

use pod_identity::{Identity, KeyName};
use pod_link::h2::H2;
use pod_link::poll::{Form, Poll};
use pod_link::quic::Quic;
use pod_link::tcp::{Keys, Tcp};
use pod_link::udp::Udp;
use pod_link::ws::Ws;
use pod_link::{Carrier, Link, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn link(way: Arc<dyn pod_core::Dial>) -> String {
    let unix = unix();
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return format!("pod-link: no runtime: {e}"),
    };
    let echoed = runtime.block_on(async {
        let (a, b) = pod_link::memory::pair(pod_link::memory::MIN_RING);
        let dialer = Link::new(a, Role::Dialer, Settings::default())?;
        let listener = Link::new(b, Role::Listener, Settings::default())?;
        let mut out = dialer.open()?;
        out.write_all(b"probe").await?;
        out.shutdown().await?;
        let mut back = listener.accept().await?;
        let mut got = Vec::new();
        back.read_to_end(&mut got).await?;
        dialer.close().await;
        Ok::<_, std::io::Error>(got.len())
    });
    let tcp = runtime.block_on(tcp(way.clone()));
    let ws = runtime.block_on(ws(way.clone()));
    let h2 = runtime.block_on(h2(way.clone()));
    let sse = runtime.block_on(poll(way));
    let quic = runtime.block_on(quic());
    let udp = runtime.block_on(udp());
    format!(
        "pod-link: {} bytes over the memory carrier, the unix carrier {unix}, the tcp carrier {tcp}, the ws carrier {ws}, the h2 carrier {h2}, the sse carrier {sse}, the quic carrier {quic}, the udp carrier {udp}",
        echoed.map_or_else(|e| e.to_string(), |n| n.to_string())
    )
}

/// A dial of the carrier to its own listener, which pins the key of this
/// end on each side.
async fn tcp(way: Arc<dyn pod_core::Dial>) -> String {
    let identity = Arc::new(Identity::from_seed(&[11; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = Tcp::new(provider, identity, way).with_clients(Keys::new([KeyName::Key(key)]));
    if let Err(e) = carrier.available() {
        return e.to_string();
    }
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@{}", key.hex(), door.local_addr());
    match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(_), Ok((_, peer))) => format!("linked to {peer} over TLS"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

/// A dial of the ws carrier to its own listener, over the accept loop of
/// pod-ws.
async fn ws(way: Arc<dyn pod_core::Dial>) -> String {
    let identity = Arc::new(Identity::from_seed(&[12; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = Ws::new(provider, identity, way).with_clients(Keys::new([KeyName::Key(key)]));
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@wss://{}/link", key.hex(), door.local_addr());
    match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(_), Ok((_, peer))) => format!("linked to {peer} over a WebSocket"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

/// A dial of the h2 carrier to its own listener, over one POST.
async fn h2(way: Arc<dyn pod_core::Dial>) -> String {
    let identity = Arc::new(Identity::from_seed(&[13; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = H2::new(provider, identity, way).with_clients(Keys::new([KeyName::Key(key)]));
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@https://{}/link", key.hex(), door.local_addr());
    match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(_), Ok((_, peer))) => format!("linked to {peer} over HTTP/2"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

/// A dial of the sse carrier (a poll carrier) to its own listener, and one
/// line down its push of SSE.
async fn poll(way: Arc<dyn pod_core::Dial>) -> String {
    let identity = Arc::new(Identity::from_seed(&[14; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = Poll::new(provider, identity, way).with_form(Form::Sse).with_clients(Keys::new([KeyName::Key(key)]));
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@https://{}/link", key.hex(), door.local_addr());
    let (mut near, (mut far, peer)) = match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(near), Ok(far)) => (near, far),
        (Err(e), _) | (_, Err(e)) => return e.to_string(),
    };
    let mut line = [0u8; 5];
    match tokio::join!(far.write_all(b"probe"), near.read_exact(&mut line)) {
        (Ok(()), Ok(_)) => format!("linked to {peer} over requests of HTTP/1.1"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

/// A dial of the quic carrier to its own listener, from a UDP socket of
/// the system.
async fn quic() -> String {
    let identity = Arc::new(Identity::from_seed(&[14; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = Quic::new(provider, identity).with_clients(Keys::new([KeyName::Key(key)]));
    if let Err(e) = carrier.available() {
        return e.to_string();
    }
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@{}", key.hex(), door.local_addr());
    match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(_), Ok((_, peer))) => format!("linked to {peer} over QUIC"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

/// A dial of the udp carrier to its own listener: KCP over UDP, TLS above.
async fn udp() -> String {
    let identity = Arc::new(Identity::from_seed(&[15; 32]));
    let key = identity.public();
    let provider = Arc::new(pod_tls::crypto::provider());
    let carrier = Udp::new(provider, identity).with_clients(Keys::new([KeyName::Key(key)]));
    if let Err(e) = carrier.available() {
        return e.to_string();
    }
    let mut door = match carrier.bind(([127, 0, 0, 1], 0).into()).await {
        Ok(door) => door,
        Err(e) => return e.to_string(),
    };
    let address = format!("{}@{}", key.hex(), door.local_addr());
    match tokio::join!(carrier.dial(&address), door.accept_key()) {
        (Ok(_), Ok((_, peer))) => format!("linked to {peer} over KCP"),
        (Err(e), _) | (_, Err(e)) => e.to_string(),
    }
}

#[cfg(unix)]
fn unix() -> String {
    pod_link::unix::Unix::default().available().map_or_else(|e| e.to_string(), |()| "available".into())
}

#[cfg(not(unix))]
fn unix() -> String {
    "not built".into()
}
