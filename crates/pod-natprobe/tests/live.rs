//! Live NAT probe test against the STUN servers listed in `pod-services` (Cloudflare and Google).
//! Marked `#[ignore]` so offline test runs never touch the network.

use pod_natprobe::run::system::resolve_pod_services_stun;
use pod_natprobe::{Prober, Profile, Seams, Settings};

#[tokio::test]
#[ignore]
async fn live_pod_services_fast_probe() {
    let (v4, v6) = resolve_pod_services_stun().await;
    assert!(!v4.is_empty(), "pod-services must resolve at least one IPv4 STUN endpoint");
    let settings = Settings::for_profile(Profile::Fast, v4).with_servers_v6(v6);
    let report = Prober::new().run(settings, Seams::system()).await.expect("live probe runs");
    assert!(report.v4.is_some());
}
