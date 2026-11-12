//! pod-warp: a MASQUE identity through its PEM form, a capsule through the
//! decoder, a table of rules, and a session to a door where nothing
//! listens, so that the key, the TLS of the door, the HTTP/2 client and the
//! stack of a session link in.

use std::sync::Arc;
use std::time::Duration;

pub fn warp(provider: Arc<rustls::crypto::CryptoProvider>, dialer: Arc<dyn pod_core::Dial>) -> String {
    let identity = match pod_warp::Identity::generate() {
        Ok(identity) => identity,
        Err(e) => return format!("pod-warp: no identity: {e}"),
    };
    let (cert, key) = identity.to_pem();
    let again =
        pod_warp::Identity::from_pem(&cert, &key).map_or_else(|e| e.to_string(), |i| i.spki_b64().len().to_string());
    let mut frame = bytes::BytesMut::new();
    pod_warp::capsule::put_capsule(&mut frame, pod_warp::capsule::CAPSULE_DATA, b"\x45probe");
    let mut decoder = pod_warp::capsule::CapsuleDecoder::new();
    decoder.push(&frame);
    let packet = decoder.next_packet().map_or(0, |p| p.len());
    let rules = pod_warp::rules::Table::build(
        &["domain:.probe.example=reject".to_string()],
        None,
        pod_warp::rules::Action::Tunnel,
        &pod_warp::rules::NoLint,
        false,
    )
    .map_or_else(|e| e.to_string(), |(t, _)| t.rules.len().to_string());
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return format!("pod-warp: no runtime: {e}"),
    };
    let mut way = pod_warp::Way::new(dialer, provider);
    way.timeout = Duration::from_millis(500);
    let door: pod_core::Target = "127.0.0.1:9".parse().expect("a door");
    let session =
        runtime.block_on(pod_warp::MasqueSession::connect(&door, &identity, identity.spki_der().to_vec(), &way));
    let session = session.map_or_else(|e| e.class.name().to_string(), |_| "open".into());
    format!("pod-warp: an SPKI of {again} characters, a packet of {packet} bytes, {rules} rules, a session {session}")
}
