//! The live check of WARP (`live.yml`): a new device at the real API, a
//! session at the real front door that `Live` holds, and the trace of
//! 1.1.1.1 over HTTPS through it with `warp=on` (podnet found no trace on
//! port 80); then the device is removed. Ignored: a test of the loopback
//! reaches no service (`docs/development.md`).

mod common;

use pod_warp::{LiveSettings, Warp};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live: the WARP API and its front door (live.yml)"]
async fn live_register_connect_and_trace() {
    let direct: std::sync::Arc<dyn pod_core::Dial> = std::sync::Arc::new(pod_proxy::Dialer::new(
        pod_proxy::Settings::new("pod-warp-live", pod_proxy::ProxyChoice::Direct),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
    ));
    let mut settings =
        pod_warp::Settings::new(pod_warp::Api::new(common::client()), pod_warp::Way::new(direct, common::provider()));
    settings.report = pod_core::Report::default();
    let warp = std::sync::Arc::new(Warp::register(settings).await.expect("a device"));
    let context = pod_tls::Context::new(common::provider(), pod_tls::Settings::new(pod_tls::Words::new("pod-warp")));
    let tls = context.client_config(&pod_tls::roots_from_compiled_set()).expect("a TLS client");
    let result = async {
        let live = warp.clone().connect_live(&LiveSettings::default()).await?;
        let net = live.get().await?;
        pod_warp::exit::trace(net.as_ref(), &tls).await
    }
    .await;
    let removed = warp.delete().await;
    let exit = result.unwrap_or_else(|e| panic!("the session: {}", e.chain()));
    assert!(matches!(exit.warp.as_deref(), Some("on" | "plus")), "{exit:?}");
    assert_eq!(exit.colo.len(), 3, "{exit:?}");
    removed.unwrap_or_else(|e| panic!("the removal of the device: {}", e.chain()));
}
