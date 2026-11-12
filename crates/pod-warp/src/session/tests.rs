use bytes::BytesMut;

use super::*;
use crate::capsule;

/// A CONNECT on `client`, for the tests of a session over an h2 server of
/// the test.
async fn connect_on(client: h2::client::SendRequest<Bytes>) -> MasqueSession {
    let mut client = client.ready().await.unwrap();
    let request = Request::builder().method(Method::CONNECT).uri(service::connect_uri()).body(()).unwrap();
    let (response, send) = client.send_request(request, false).unwrap();
    let response = response.await.unwrap();
    MasqueSession::from_h2(None, send, response.into_body())
}

/// The front door sends one small DATA frame for each packet (ACKs). A full
/// stream window of 59-byte frames, with a reader that waits before it
/// reads: the session must not close with ENHANCE_YOUR_CALM
/// (PodUtils/podnet issue 32: too_many_data_frames after 4 MiB).
#[tokio::test]
async fn many_small_data_frames_do_not_close_the_session() {
    let (client_io, server_io) = tokio::io::duplex(1 << 20);
    let server = tokio::spawn(async move {
        let mut conn = h2::server::handshake(server_io).await.unwrap();
        let (_request, mut respond) = conn.accept().await.unwrap().unwrap();
        let response = http::Response::builder().status(200).body(()).unwrap();
        let mut send = respond.send_response(response, false).unwrap();
        let sender = tokio::spawn(async move {
            let mut frame = BytesMut::new();
            capsule::put_capsule(&mut frame, capsule::CAPSULE_DATA, &[0x45; 57]);
            let frame = frame.freeze();
            for _ in 0..STREAM_WINDOW as usize / frame.len() {
                send.reserve_capacity(frame.len());
                while send.capacity() < frame.len() {
                    std::future::poll_fn(|cx| send.poll_capacity(cx)).await.unwrap().unwrap();
                }
                send.send_data(frame.clone(), false).unwrap();
            }
            send.send_data(Bytes::new(), true).unwrap();
        });
        while let Some(next) = conn.accept().await {
            drop(next);
        }
        sender.await.unwrap();
    });
    let (client, connection) = h2_builder().handshake(client_io).await.unwrap();
    tokio::spawn(connection);
    let session = connect_on(client).await;
    let (_tx, mut rx) = session.split();
    // A slow receive task: the frames pile up in h2 first.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut packets = 0;
    while let Some(_packet) = rx.recv_packet().await.unwrap() {
        packets += 1;
    }
    assert_eq!(packets, STREAM_WINDOW as usize / 59);
    server.abort();
}

/// A client with a PING handle, and a server that answers the request and
/// then drives its connection (`answer_pings`) or holds it still.
async fn keepalive_pair(answer_pings: bool) -> (Keepalive, tokio::task::JoinHandle<()>, Box<dyn std::any::Any + Send>) {
    let (client_io, server_io) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(async move {
        let mut conn = h2::server::Builder::new().handshake::<_, Bytes>(server_io).await.unwrap();
        let (_request, mut respond) = conn.accept().await.unwrap().unwrap();
        let response = http::Response::builder().status(200).body(()).unwrap();
        let _send = respond.send_response(response, false).unwrap();
        if answer_pings {
            while let Some(next) = conn.accept().await {
                drop(next);
            }
        } else {
            // The connection is not polled: no pong is sent.
            std::future::pending::<()>().await;
            drop(conn);
        }
    });
    let (client, mut connection) = h2::client::handshake(client_io).await.unwrap();
    let ping = connection.ping_pong().unwrap();
    tokio::spawn(connection);
    let mut client = client.ready().await.unwrap();
    let request = Request::builder().method(Method::CONNECT).uri(service::connect_uri()).body(()).unwrap();
    let (response, send) = client.send_request(request, false).unwrap();
    let response = tokio::time::timeout(Duration::from_millis(500), response).await;
    let keepalive = Keepalive::h2(ping, Default::default());
    // The client, the request stream and the response stay alive: the
    // connection closes when they are dropped.
    (keepalive, server, Box::new((client, send, response)))
}

/// PodUtils/podnet issue 104: many PINGs in a row (before, the second one
/// failed: the pong was never collected).
#[tokio::test]
async fn the_keepalive_pings_many_times() {
    let (keepalive, server, _keep) = keepalive_pair(true).await;
    let run = tokio::time::timeout(Duration::from_millis(500), keepalive.run(Duration::from_millis(20))).await;
    assert!(run.is_err(), "the keepalive ended: {run:?}");
    server.abort();
}

/// PodUtils/podnet issue 104: a peer that gives no pong and no data is a
/// dead session.
#[tokio::test]
async fn no_pong_ends_the_session() {
    let (keepalive, server, _keep) = keepalive_pair(false).await;
    let run = tokio::time::timeout(Duration::from_secs(5), keepalive.run(Duration::from_millis(20)))
        .await
        .expect("the keepalive ends");
    let err = run.unwrap_err();
    assert!(err.to_string().contains("no pong and no data"), "{err}");
    assert_eq!(err.class, Class::Timeout);
    server.abort();
}

/// Data from the peer is proof of life: no PING is needed.
#[tokio::test]
async fn incoming_data_keeps_the_session_alive() {
    let (keepalive, server, _keep) = keepalive_pair(false).await;
    let progress = keepalive.progress();
    let feeder = tokio::spawn(async move {
        loop {
            progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    let run = tokio::time::timeout(Duration::from_millis(500), keepalive.run(Duration::from_millis(20))).await;
    assert!(run.is_err(), "the keepalive ended: {run:?}");
    feeder.abort();
    server.abort();
}

// PodUtils/podnet issue 122: ten packets of send_packets go in one DATA
// frame, and decode in order.
#[tokio::test]
async fn packets_of_one_batch_go_in_one_data_frame() {
    let (client_io, server_io) = tokio::io::duplex(1 << 20);
    let (chunks_tx, chunks_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut conn = h2::server::Builder::new().handshake::<_, Bytes>(server_io).await.unwrap();
        let (request, mut respond) = conn.accept().await.unwrap().unwrap();
        let response = http::Response::builder().status(200).body(()).unwrap();
        let _send = respond.send_response(response, false).unwrap();
        tokio::spawn(async move { while conn.accept().await.is_some() {} });
        let mut body = request.into_body();
        let mut chunks = 0;
        let mut decoder = CapsuleDecoder::new();
        let mut packets = Vec::new();
        while packets.len() < 10 {
            let data = body.data().await.unwrap().unwrap();
            let _ = body.flow_control().release_capacity(data.len());
            chunks += 1;
            decoder.push(&data);
            while let Some(p) = decoder.next_packet() {
                packets.push(p[0]);
            }
        }
        let _ = chunks_tx.send((chunks, packets));
    });
    let (client, connection) = h2::client::handshake(client_io).await.unwrap();
    tokio::spawn(connection);
    let (mut tx, _rx) = connect_on(client).await.split();
    let batch: Vec<Vec<u8>> = (0..10u8).map(|i| vec![i; 1200]).collect();
    tx.send_packets(&batch).await.unwrap();
    let (chunks, packets) = chunks_rx.await.unwrap();
    assert_eq!(packets, (0..10u8).collect::<Vec<_>>());
    assert!(chunks <= 2, "{chunks} DATA chunks");
}

/// A front door that never reads the CONNECT-IP body: the stream window
/// fills, and then the sender waits (PodUtils/podnet issue 93: no packet is
/// dropped here, and h2 holds no unbounded buffer).
#[tokio::test]
async fn a_full_window_makes_the_sender_wait() {
    let (client_io, server_io) = tokio::io::duplex(1 << 20);
    let server = tokio::spawn(async move {
        let mut conn = h2::server::Builder::new()
            .initial_window_size(16 * 1024)
            .initial_connection_window_size(16 * 1024)
            .handshake::<_, Bytes>(server_io)
            .await
            .unwrap();
        let (_request, mut respond) = conn.accept().await.unwrap().unwrap();
        let response = http::Response::builder().status(200).body(()).unwrap();
        let _send = respond.send_response(response, false).unwrap();
        // Drive the connection, and never read the request body.
        while let Some(next) = conn.accept().await {
            drop(next);
        }
    });
    let (client, connection) = h2::client::handshake(client_io).await.unwrap();
    tokio::spawn(connection);
    let (mut tx, _rx) = connect_on(client).await.split();
    let packet = vec![0x45u8; 1200];
    // The first packets of a new stream go out at once (a QUIC Initial must
    // not wait for a retransmission).
    for _ in 0..8 {
        tx.send_packet(&packet).await.unwrap();
    }
    assert_eq!(tx.waits(), 0);
    // 16 KiB of window: after it, a packet waits, and no error comes.
    let mut sent = 8;
    let full = loop {
        match tokio::time::timeout(Duration::from_millis(200), tx.send_packet(&packet)).await {
            Ok(result) => {
                result.unwrap();
                sent += 1;
                assert!(sent < 2000, "the sender never waited");
            }
            Err(_) => break sent,
        }
    };
    assert!(tx.waits() > 0);
    // The window and SEND_RESERVE bound what went out.
    assert!(full * 1203 <= 16 * 1024 + SEND_RESERVE + 1203, "sent {full}");
    server.abort();
}
