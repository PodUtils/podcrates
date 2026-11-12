//! pod-devtunnel: a client of the stand-in on the loopback opens a stream
//! to the echo of the tunnel's host, so that the management client, the
//! relay WebSocket, the SSH client and the SSH server of the stand-in
//! (russh on ring) link in.

use std::sync::Arc;

use pod_devtunnel::standin::{Plan, Standin};
use pod_devtunnel::{Base, Locator, Relay, Scheme, Settings, UserToken, Way};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn devtunnel(provider: Arc<rustls::crypto::CryptoProvider>, dial: Arc<dyn pod_core::Dial>) -> String {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return format!("pod-devtunnel: no runtime: {e}"),
    };
    let echoed = runtime.block_on(async {
        // A made-up token of the stand-in, not a credential.
        let user = UserToken::new(Scheme::Github, "static-probe").map_err(|e| e.chain())?;
        let standin = Standin::start(Plan::echo("github", "static-probe")).await.map_err(|e| e.to_string())?;
        let tls =
            Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe"))));
        let http = pod_proxy::Client::new(tls.clone(), dial.clone()).with_user_agent("static-probe");
        let way = Way { http, ws: pod_ws::Way::new(tls, dial, "static-probe"), trust: pod_tls::Trust::Default };
        let settings =
            Settings { management: Base::parse(&standin.base()).map_err(|e| e.chain())?, ..Settings::default() };
        let locator = Locator::parse("standin-box").map_err(|e| e.chain())?;
        let relay =
            Relay::connect(way, settings, locator, user, pod_core::Report::default()).await.map_err(|e| e.chain())?;
        let mut stream = relay.open(22).await.map_err(|e| e.chain())?;
        stream.write_all(b"probe").await.map_err(|e| e.to_string())?;
        let mut back = [0u8; 5];
        stream.read_exact(&mut back).await.map_err(|e| e.to_string())?;
        relay.close().await;
        Ok::<_, String>(back.len())
    });
    format!(
        "pod-devtunnel: {} bytes through the stand-in's relay, the management API {}",
        echoed.map_or_else(|e| e, |n| n.to_string()),
        Settings::default().management.host
    )
}
