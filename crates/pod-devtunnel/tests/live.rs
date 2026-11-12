//! The live service, with the token of the user who runs the test: never a
//! shared account, never a token in the code. The test is ignored; it runs
//! by hand with these variables, and it is skipped when one is missing:
//!
//! * `POD_DEVTUNNEL_TOKEN`: the user's token;
//! * `POD_DEVTUNNEL_SCHEME`: `github` (the default) or `aad`;
//! * `POD_DEVTUNNEL_TUNNEL`: the tunnel, `NAME` or `ID.CLUSTER`, with a
//!   host connected;
//! * `POD_DEVTUNNEL_PORT`: a port of the tunnel whose server speaks first
//!   (an SSH server).
//!
//! The test reads the variables and sets none.

use std::sync::Arc;
use std::time::Duration;

use pod_devtunnel::{Locator, Relay, Scheme, Settings, UserToken, Way};
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, Environment, ProxyChoice};
use pod_tls::{Context, Trust, Words};
use tokio::io::AsyncReadExt;

#[tokio::test]
#[ignore = "the live service of Dev Tunnels, with the user's own token"]
async fn live_a_stream_to_a_port_of_the_users_tunnel() {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let (Some(token), Some(tunnel), Some(port)) =
        (var("POD_DEVTUNNEL_TOKEN"), var("POD_DEVTUNNEL_TUNNEL"), var("POD_DEVTUNNEL_PORT"))
    else {
        eprintln!("skipped: POD_DEVTUNNEL_TOKEN, POD_DEVTUNNEL_TUNNEL and POD_DEVTUNNEL_PORT are not all set");
        return;
    };
    let scheme = Scheme::parse(&var("POD_DEVTUNNEL_SCHEME").unwrap_or_else(|| "github".into())).expect("a scheme");
    let user = UserToken::new(scheme, token).expect("a token that a header carries");
    let port: u16 = port.parse().expect("a port");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-devtunnel"))));
    let environment = Environment::read(&|name| std::env::var_os(name));
    let settings = pod_proxy::Settings::new("pod-devtunnel", ProxyChoice::FromEnvironment(environment));
    let dialer: Arc<dyn pod_core::Dial> = Arc::new(Dialer::new(settings, Arc::new(System::new())));
    let way = Way {
        http: Client::new(tls.clone(), dialer.clone()).with_user_agent("pod-devtunnel-live"),
        ws: pod_ws::Way::new(tls, dialer, "pod-devtunnel-live"),
        trust: Trust::Default,
    };
    let locator = Locator::parse(&tunnel).expect("a tunnel");
    let relay = Relay::connect(way, Settings::default(), locator, user, pod_core::Report::default())
        .await
        .unwrap_or_else(|e| panic!("connect: {}", e.chain()));
    let mut stream = relay.open(port).await.unwrap_or_else(|e| panic!("open: {}", e.chain()));
    let mut first = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut first))
        .await
        .expect("the server speaks within 20 s")
        .expect("the first bytes of the server");
    assert_eq!(&first, b"SSH-", "the port's server is an SSH server");
    relay.reconnect().await.unwrap_or_else(|e| panic!("reconnect: {}", e.chain()));
    assert_eq!(relay.sessions(), 2);
    relay.close().await;
}
