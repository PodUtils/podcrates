//! The hooks of the consumer: the admit of a target and the UDP service at
//! the gateway.

use std::sync::Mutex;

use super::*;

/// A stand-in resolver: it answers the name `web` with 10.66.0.2 and gives
/// no answer to another request. The answer is the query id, then the four
/// bytes of the address: the stack does not read the bytes.
struct Names;

impl GatewayService for Names {
    fn port(&self) -> u16 {
        53
    }

    fn answer(&self, _from: SocketAddr, request: &[u8]) -> Option<Vec<u8>> {
        let (id, name) = request.split_at_checked(2)?;
        (name == b"web").then(|| [id, &[10, 66, 0, 2]].concat())
    }
}

/// The UDP service of the consumer answers at the gateway's port 53
/// (podnet: its resolver); the answer comes from the gateway.
#[tokio::test]
async fn virtual_dns_answers_at_the_gateway() {
    let config = StackConfig { gateway_service: Some(Arc::new(Names)), ..StackConfig::default() };
    let mut net = NetStack::loopback_with(config);
    let mut client = net.bind_udp(at(LOCAL_IP, 40053)).await.unwrap();
    let gateway = at("10.66.0.1".parse().unwrap(), 53);
    client.send_to(b"\x12\x34other", gateway).await.unwrap();
    client.send_to(b"\x12\x34web", gateway).await.unwrap();
    let mut buffer = [0u8; 512];
    let (n, from) = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buffer))
        .await
        .expect("dns timed out")
        .unwrap();
    assert_eq!(from, gateway);
    assert_eq!(&buffer[..n], &[0x12, 0x34, 10, 66, 0, 2], "the request with no answer gets nothing");
    net.shutdown().await;
}

/// An admit that refuses each target refuses a connect, and the stack
/// counts the refusal.
#[tokio::test]
async fn acl_refuses_connections() {
    let deny = |_: SocketAddr| false;
    let config = StackConfig { admit: Some(Arc::new(deny)), ..StackConfig::default() };
    let mut net = NetStack::loopback_with(config);
    let mut events = net.take_events().unwrap();
    let err = net.connect(at(LOCAL_IP, 8080)).await.unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(net.stats().refused, 1);
    assert!(matches!(events.recv().await, Some(StackEvent::Refused { remote, .. }) if remote == at(LOCAL_IP, 8080)));
    net.shutdown().await;
}

/// The admit sees the target of each listen, connect, UDP bind and UDP send.
/// A refused bind is `PermissionDenied`; a refused datagram is dropped and
/// counted, and the next datagram goes.
#[tokio::test]
async fn the_admit_sees_each_target() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let admit = {
        let seen = seen.clone();
        move |target: SocketAddr| {
            seen.lock().unwrap().push(target);
            target.port() != 7
        }
    };
    let config = StackConfig { admit: Some(Arc::new(admit)), ..StackConfig::default() };
    let mut net = NetStack::loopback_with(config);
    let listen = at(LOCAL_IP, 8080);
    let _listener = net.listen(listen).await.unwrap();
    assert_eq!(net.listen(at(LOCAL_IP, 7)).await.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(net.bind_udp(at(LOCAL_IP, 7)).await.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    let _client = net.connect(listen).await.unwrap();
    let mut server = net.bind_udp(at(LOCAL_IP, 5300)).await.unwrap();
    let client = net.bind_udp(at(LOCAL_IP, 5301)).await.unwrap();
    client.send_to(b"refused", at(LOCAL_IP, 7)).await.unwrap();
    client.send_to(b"admitted", at(LOCAL_IP, 5300)).await.unwrap();
    let mut buffer = [0u8; 16];
    let (n, _) = tokio::time::timeout(Duration::from_secs(2), server.recv_from(&mut buffer))
        .await
        .expect("udp timed out")
        .unwrap();
    assert_eq!(&buffer[..n], b"admitted");
    assert_eq!(net.stats().refused, 1, "the refused datagram is counted");
    let seen = seen.lock().unwrap().clone();
    for target in [listen, at(LOCAL_IP, 7), at(LOCAL_IP, 5300), at(LOCAL_IP, 5301)] {
        assert!(seen.contains(&target), "the admit did not see {target}: {seen:?}");
    }
    net.shutdown().await;
}
