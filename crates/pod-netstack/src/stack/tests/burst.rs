//! A burst of SYNs to one port (T-232, PodUtils/podnet#457): each SYN finds
//! a listen socket, no SYN gets a RST, a full accept queue drops no
//! connection, and a SYN past `max_connections` waits. A SYN sent again
//! reaches its own socket (T-223).

use std::sync::atomic::AtomicUsize;

use super::*;

/// A server at 10.66.0.2 and a client at 10.66.0.3 over one link. The pump
/// from the client holds each packet until the gate opens, so that a burst
/// of SYNs comes in at one time; `drop` drops the packets of the server
/// that it returns true for.
pub(super) fn gated(
    server: StackConfig,
    drop: impl Fn(&[u8]) -> bool + Send + 'static,
) -> (NetStack, NetStack, Arc<Notify>, Vec<tokio::task::JoinHandle<()>>) {
    let client = StackConfig { local_v4: Ipv4Addr::new(10, 66, 0, 3), ..StackConfig::default() };
    let (server, client) = (NetStack::over_link(server), NetStack::over_link(client));
    let gate = Arc::new(Notify::new());
    let (from, to, open) = (client.link_out(), server.link_in(), gate.clone());
    let up = tokio::spawn(async move {
        open.notified().await;
        loop {
            to.push(from.recv().await);
        }
    });
    let (from, to) = (server.link_out(), client.link_in());
    let down = tokio::spawn(async move {
        loop {
            let packet = from.recv().await;
            if !drop(&packet) {
                to.push(packet);
            }
        }
    });
    (server, client, gate, vec![up, down])
}

pub(super) async fn stop(mut server: NetStack, mut client: NetStack, pumps: Vec<tokio::task::JoinHandle<()>>) {
    for pump in pumps {
        pump.abort();
    }
    server.shutdown().await;
    client.shutdown().await;
}

/// A filter of the pump that drops nothing and counts each RST of the server.
pub(super) fn counting(resets: Arc<AtomicUsize>) -> impl Fn(&[u8]) -> bool + Send + 'static {
    move |packet| {
        if is_reset(packet) {
            resets.fetch_add(1, Ordering::Relaxed);
        }
        false
    }
}

/// Answer each accepted stream: read one byte, write it back.
fn echo(mut listener: TcpListener, count: usize) -> tokio::task::JoinHandle<TcpListener> {
    tokio::spawn(async move {
        for _ in 0..count {
            let mut stream = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut one = [0u8; 1];
                if stream.read_exact(&mut one).await.is_ok() {
                    let _ = stream.write_all(&one).await;
                }
            });
        }
        listener
    })
}

/// Connect `count` clients, then open the gate: each SYN reaches the server
/// in one burst.
async fn burst(client: &NetStack, gate: &Notify, addr: SocketAddr, count: usize) -> Vec<TcpStream> {
    let mut clients = Vec::new();
    for _ in 0..count {
        clients.push(client.connect(addr).await.unwrap());
    }
    gate.notify_one();
    clients
}

/// Each client connects, with no RST.
async fn handshakes(clients: &mut [TcpStream], limit: Duration) {
    for (k, client) in clients.iter_mut().enumerate() {
        let handshake = tokio::time::timeout(limit, client.established()).await;
        handshake.expect("no handshake in the time limit").unwrap_or_else(|err| panic!("client {k}: {err}"));
    }
}

/// Each client gets its byte back.
async fn each_answers(clients: &mut [TcpStream], limit: Duration) {
    for (k, client) in clients.iter_mut().enumerate() {
        client.write_all(&[k as u8]).await.unwrap();
        let mut one = [0u8; 1];
        tokio::time::timeout(limit, client.read_exact(&mut one)).await.expect("no answer").unwrap();
        assert_eq!(one[0], k as u8);
    }
}

/// 48 connects at once to one port with the default settings: each
/// connects, with no RST, and each gets its answer (podnet's
/// `netns_backend_takes_a_burst_of_connects_to_one_port`).
#[tokio::test]
async fn a_burst_of_connects_to_one_port_gets_no_reset() {
    const COUNT: usize = 48;
    let (server, client, gate, pumps) = gated(StackConfig::default(), |_| false);
    let addr = at(LOCAL_IP, 443);
    let echo = echo(server.listen(addr).await.unwrap(), COUNT);
    let mut clients = burst(&client, &gate, addr, COUNT).await;
    handshakes(&mut clients, Duration::from_secs(5)).await;
    each_answers(&mut clients, Duration::from_secs(5)).await;
    let _listener = echo.await.unwrap();
    assert_eq!(server.stats().accepted, COUNT as u64);
    assert_eq!(server.stats().dropped, 0);
    stop(server, client, pumps).await;
}

/// 100 connects at once, more than the accept queue holds, and an app that
/// accepts after each handshake: no connection is lost, and no RST crosses
/// the link (PodUtils/podnet#464).
#[tokio::test]
async fn a_burst_past_the_accept_queue_loses_no_connection() {
    const COUNT: usize = 100;
    let resets = Arc::new(AtomicUsize::new(0));
    let (server, client, gate, pumps) = gated(StackConfig::default(), counting(resets.clone()));
    let addr = at(LOCAL_IP, 443);
    let listener = server.listen(addr).await.unwrap();
    let mut clients = burst(&client, &gate, addr, COUNT).await;
    handshakes(&mut clients, Duration::from_secs(5)).await;
    let echo = echo(listener, COUNT);
    each_answers(&mut clients, Duration::from_secs(5)).await;
    let _listener = echo.await.unwrap();
    assert_eq!(server.stats().accepted, COUNT as u64);
    assert_eq!(resets.load(Ordering::Relaxed), 0, "a RST crossed the link");
    stop(server, client, pumps).await;
}

/// A server that holds `max_connections`: a SYN past it gets no RST. It
/// waits, and connects after the connections of the server close.
#[tokio::test]
async fn a_syn_past_the_connection_limit_waits() {
    const LIMIT: usize = 4;
    let config = StackConfig { max_connections: LIMIT, ..StackConfig::default() };
    let resets = Arc::new(AtomicUsize::new(0));
    let (server, client, gate, pumps) = gated(config, counting(resets.clone()));
    let addr = at(LOCAL_IP, 443);
    let mut listener = server.listen(addr).await.unwrap();
    let mut clients = burst(&client, &gate, addr, 2 * LIMIT).await;
    let mut first = Vec::new();
    for _ in 0..LIMIT {
        first.push(tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.expect("no accept").unwrap());
    }
    let mut waiting = Vec::new();
    for mut client in clients.drain(..) {
        match tokio::time::timeout(Duration::ZERO, client.established()).await {
            Err(_) => waiting.push(client),
            Ok(handshake) => handshake.expect("a client got a RST at the limit"),
        }
    }
    assert_eq!(waiting.len(), LIMIT, "the server took more than its limit");
    assert_eq!(server.stats().dropped, LIMIT as u64);
    assert_eq!(resets.load(Ordering::Relaxed), 0, "a RST crossed the link at the limit");
    // Each end of the first connections closes; the SYNs that the clients
    // send again connect.
    drop(first);
    let echo = echo(listener, LIMIT);
    handshakes(&mut waiting, Duration::from_secs(10)).await;
    each_answers(&mut waiting, Duration::from_secs(5)).await;
    let _listener = echo.await.unwrap();
    assert_eq!(server.stats().accepted, 2 * LIMIT as u64);
    stop(server, client, pumps).await;
}

/// The first SYN-ACK of a connection is lost, and a free slot of the set
/// comes before the socket of its handshake: the listen socket that the
/// listener adds there must not take the SYN that the client sends again
/// (T-223). The connection works, and each socket leaves the set after it.
#[tokio::test]
async fn a_syn_sent_again_reaches_its_own_socket() {
    let lost = Arc::new(AtomicUsize::new(0));
    let count = lost.clone();
    // A TCP SYN-ACK in an IPv4 packet with no options.
    let first_syn_ack = move |packet: &[u8]| {
        packet.len() >= 40 && packet[9] == 6 && packet[33] & 0x12 == 0x12 && count.fetch_add(1, Ordering::Relaxed) == 0
    };
    let (server, client, gate, pumps) = gated(StackConfig::default(), first_syn_ack);
    gate.notify_one();
    // The listener on port 1 takes the first slot of the set, and frees it.
    let first = server.listen(at(LOCAL_IP, 1)).await.unwrap();
    let mut listener = server.listen(at(LOCAL_IP, 443)).await.unwrap();
    drop(first);
    // One free listen socket of the listener on port 443.
    let resting = 1;
    assert_eq!(settle(&server, resting).await, resting);
    let mut stream = client.connect(at(LOCAL_IP, 443)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), stream.established()).await.expect("no handshake").unwrap();
    assert!(lost.load(Ordering::Relaxed) >= 2, "the pump saw no second SYN-ACK");
    let mut accepted =
        tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.expect("no accept").unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), accepted.read_exact(&mut got)).await.expect("no data").unwrap();
    assert_eq!(&got, b"ping");
    drop(accepted);
    drop(stream);
    assert_eq!(settle(&server, resting).await, resting, "a socket of the handshake stays");
    stop(server, client, pumps).await;
}
