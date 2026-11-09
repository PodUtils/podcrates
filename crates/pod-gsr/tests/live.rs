//! The live check through the relay of hackerschoice/gsocket.
//!
//! The test needs the egress of the host: it reads `HTTPS_PROXY`, `ALL_PROXY`
//! and `NO_PROXY` through `pod-proxy`. Run it by hand:
//!
//! ```sh
//! cargo test -p pod-gsr --test live -- --ignored
//! ```

use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::OsEntropy;
use pod_gsr::{Role, Settings, open};
use pod_proxy::{Dialer, Environment, ProxyChoice, Settings as ProxySettings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

/// A made-up secret of this test. It is not a credential.
const SECRET: &str = "pod-gsr-live-check-1";
/// The bytes of each direction.
const BYTES: usize = 1 << 20;

#[tokio::test]
#[ignore = "the live relay of hackerschoice/gsocket"]
async fn one_megabyte_crosses_the_real_relay() {
    let environment = Environment::read(&|name| std::env::var_os(name));
    let proxy = ProxySettings::new("pod-gsr", ProxyChoice::FromEnvironment(environment));
    let dialer = Arc::new(Dialer::new(proxy, Arc::new(pod_proxy::resolve::System::new())));
    let settings = Settings::of_data().expect("the data file");

    let listener = tokio::spawn({
        let settings = settings.clone();
        let dialer = Arc::clone(&dialer);
        async move {
            let mut entropy = OsEntropy;
            open(&*dialer, &settings, Role::Listen, SECRET, &mut entropy, Instant::now() + Duration::from_secs(120))
                .await
        }
    });
    // Let the registration arrive before the connector asks for it.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut entropy = OsEntropy;
    let mut connector =
        open(&*dialer, &settings, Role::Connect, SECRET, &mut entropy, Instant::now() + Duration::from_secs(60))
            .await
            .expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    let payload: Vec<u8> = (0u8..=255).cycle().take(BYTES).collect();
    let mut received = vec![0u8; BYTES];
    let (written, read) = tokio::join!(async { connector.write_all(&payload).await }, async {
        listener.read_exact(&mut received).await
    },);
    written.expect("the payload");
    read.expect("the payload arrives");
    assert_eq!(received, payload, "the bytes of the tunnel are the caller's bytes");
}
