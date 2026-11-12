//! pod-turn: the codec signs and checks a request with the SHA-256 key,
//! and the client tries an allocation on a closed port of the loopback
//! with short limits, so that the codec, the client, its UDP source and its
//! errors link in.

use std::time::Duration;

use pod_turn::stun::{self, PasswordAlgorithm};

pub fn turn() -> String {
    let key = PasswordAlgorithm::Sha256.key("probe", "probe.example", "probe");
    let request = stun::message(stun::message_type(stun::ALLOCATE, stun::Class::Request), &[7; 12], &[]);
    let signed = stun::signed_sha256(&request, &key);
    let checked = stun::integrity_sha256(&signed, &key).map_or_else(|e| e.to_string(), |()| "holds".into());
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return format!("pod-turn: no runtime: {e}"),
    };
    let tried = runtime.block_on(async {
        let server = pod_core::Target::Addr(std::net::SocketAddr::from(([127, 0, 0, 1], 9)));
        let source = std::sync::Arc::new(pod_turn::SystemUdp::new());
        let mut settings = pod_turn::Settings::udp(server, pod_turn::Credentials::new("probe", "probe"), source);
        settings.limits.rto = Duration::from_millis(10);
        settings.limits.retransmits = 1;
        settings.limits.request_timeout = Duration::from_millis(50);
        pod_turn::Client::allocate(settings).await.map_or_else(|e| e.to_string(), |c| c.relayed_address().to_string())
    });
    format!("pod-turn: the SHA-256 integrity {checked}; an allocation on a closed port: {tried}")
}
