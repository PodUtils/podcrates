//! The live check of WARP (`live.yml`): a new device at the real API, a
//! session at the real front door, and the trace of 1.1.1.1 through it with
//! `warp=on`; then the device is removed. Ignored: a test of the loopback
//! reaches no service (`docs/development.md`).

mod common;

use std::time::Duration;

use pod_core::{Dial, Target};
use pod_warp::Warp;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live: the WARP API and its front door (live.yml)"]
async fn live_register_connect_and_trace() {
    let direct: std::sync::Arc<dyn Dial> = std::sync::Arc::new(pod_proxy::Dialer::new(
        pod_proxy::Settings::new("pod-warp-live", pod_proxy::ProxyChoice::Direct),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
    ));
    let mut settings =
        pod_warp::Settings::new(pod_warp::Api::new(common::client()), pod_warp::Way::new(direct, common::provider()));
    settings.report = pod_core::Report::default();
    let warp = Warp::register(settings).await.expect("a device");
    let result = async {
        let net = warp.connect().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut connected = net.dial(&Target::new("1.1.1.1", 80), deadline).await?;
        let request = b"GET /cdn-cgi/trace HTTP/1.1\r\nHost: 1.1.1.1\r\nConnection: close\r\n\r\n";
        connected.io.write_all(request).await.map_err(|e| pod_core::Failure::io(pod_core::Stage::Warp, e))?;
        let mut answer = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), connected.io.read_to_end(&mut answer))
            .await
            .map_err(|_| pod_core::Failure::new(pod_core::Stage::Warp, pod_core::Class::Timeout, "no trace"))?
            .map_err(|e| pod_core::Failure::io(pod_core::Stage::Warp, e))?;
        Ok::<_, pod_core::Failure>(String::from_utf8_lossy(&answer).into_owned())
    }
    .await;
    let removed = warp.delete().await;
    let trace = result.unwrap_or_else(|e| panic!("the session: {}", e.chain()));
    assert!(trace.contains("warp=on") || trace.contains("warp=plus"), "{trace}");
    removed.unwrap_or_else(|e| panic!("the removal of the device: {}", e.chain()));
}
