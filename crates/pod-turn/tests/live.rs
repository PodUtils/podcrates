//! A live allocation on a TURN server of the caller: Allocate, a Refresh,
//! a permission and a channel to a peer of the caller, and the close. No
//! server is written here: the variables of the test name it, and the test
//! skips with a line when they do not.
//!
//! * `POD_TURN_LIVE_SERVER`: `udp://HOST:PORT` or `tls://HOST:PORT`.
//! * `POD_TURN_LIVE_USER`, `POD_TURN_LIVE_PASSWORD`: the credentials. The
//!   test never prints them.
//! * `POD_TURN_LIVE_PEER` (optional): `HOST:PORT` of a UDP echo that the
//!   relay can reach; with it, a Send and a ChannelData echo.
//!
//! `cargo test -p pod-turn --test live -- --ignored`

use std::sync::Arc;
use std::time::Duration;

use pod_turn::{Client, Credentials, Event, Settings, SystemUdp};

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
#[ignore = "a TURN server of the caller; the live workflow names it"]
async fn live() {
    let (Some(server), Some(user), Some(password)) =
        (var("POD_TURN_LIVE_SERVER"), var("POD_TURN_LIVE_USER"), var("POD_TURN_LIVE_PASSWORD"))
    else {
        println!("skipped: POD_TURN_LIVE_SERVER, POD_TURN_LIVE_USER and POD_TURN_LIVE_PASSWORD name no server");
        return;
    };
    let credentials = Credentials::new(user, password);
    let settings = if let Some(rest) = server.strip_prefix("udp://") {
        let target: pod_core::Target = rest.parse().expect("udp://HOST:PORT");
        let resolver: Arc<dyn pod_core::Resolve> = Arc::new(System);
        Settings::udp(target, credentials, Arc::new(SystemUdp::with_resolver(resolver)))
    } else if let Some(rest) = server.strip_prefix("tls://") {
        let target: pod_core::Target = rest.parse().expect("tls://HOST:PORT");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let context = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-turn-live")));
        Settings::tls(target, credentials, Arc::new(System), Arc::new(context))
    } else {
        panic!("POD_TURN_LIVE_SERVER is udp://HOST:PORT or tls://HOST:PORT");
    };
    let mut client = Client::allocate(settings).await.expect("an allocation");
    println!("relayed {} for {} s", client.relayed_address(), client.lifetime().as_secs());
    client.refresh().await.expect("a refresh");
    if let Some(peer) = var("POD_TURN_LIVE_PEER") {
        let peer = tokio::net::lookup_host(&peer).await.expect("the peer's address").next().expect("an address");
        client.create_permission(peer).await.expect("a permission");
        client.send_to(peer, b"pod-turn live").await.expect("a Send");
        let event = tokio::time::timeout(Duration::from_secs(10), client.recv()).await.expect("an echo").unwrap();
        assert!(matches!(event, Event::Data { channel: None, .. }), "{event:?}");
        let number = client.bind_channel(peer).await.expect("a channel");
        client.send(number, b"pod-turn live").await.expect("ChannelData");
        let event = tokio::time::timeout(Duration::from_secs(10), client.recv()).await.expect("an echo").unwrap();
        assert!(matches!(event, Event::Data { channel: Some(n), .. } if n == number), "{event:?}");
    }
    client.close().await.expect("the close");
}

/// The system's resolver and a direct TCP dial: the live test runs on a
/// runner with no proxy.
struct System;

impl pod_core::Resolve for System {
    fn label(&self) -> String {
        "system".into()
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        _deadline: tokio::time::Instant,
        _after: Option<pod_core::Source>,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Found, pod_core::Failure>> {
        Box::pin(async move {
            let addrs: Vec<_> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| pod_core::Failure::io(pod_core::Stage::Dns, e))?
                .collect();
            Ok(pod_core::Found { addrs, source: pod_core::Source { rank: 0, name: "system" }, more: false })
        })
    }
}

impl pod_core::Dial for System {
    fn label(&self) -> String {
        "direct".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a pod_core::Target,
        _deadline: tokio::time::Instant,
    ) -> pod_core::BoxFuture<'a, Result<pod_core::Connected, pod_core::Failure>> {
        Box::pin(async move {
            let tcp = tokio::net::TcpStream::connect(target.to_string())
                .await
                .map_err(|e| pod_core::Failure::io(pod_core::Stage::Connect, e))?;
            Ok(pod_core::Connected::new(Box::new(tcp)))
        })
    }
}
