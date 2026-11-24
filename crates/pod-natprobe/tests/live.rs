//! A live NAT probe against the STUN servers of the catalog of `pod-services` (each service of
//! the kind `stun`). The test is `#[ignore]`: an offline run of the tests sends no packet.

use std::sync::Arc;

use pod_natprobe::run::system::resolve_pod_services_stun;
use pod_natprobe::{Prober, Profile, Seams, Settings};

#[tokio::test]
#[ignore]
async fn live_pod_services_fast_probe() {
    let (v4, v6) =
        resolve_pod_services_stun(&pod_services::Catalog::new(), Arc::new(pod_proxy::resolve::System::new())).await;
    assert!(!v4.is_empty(), "pod-services must resolve at least one IPv4 STUN endpoint");
    let settings = Settings::for_profile(Profile::Fast, v4).with_servers_v6(v6);
    let report = Prober::new().run(settings, Seams::system()).await.expect("live probe runs");
    println!("{report}");
    assert!(report.v4.is_some());
}
