//! The TCP relay of RFC 6062 against coturn, a server that this crate did
//! not write: a TCP allocation over plain TCP, Connect to a peer on the
//! loopback, ConnectionBind on the second connection, and bytes both
//! ways. coturn relays a bare TCP stream, as RFC 6062 specifies, and the
//! peer here is a bare echo peer.
//!
//! `scripts/ci/turn-tests.sh` starts coturn with its TCP listener and names
//! it in `POD_TURN_TEST_COTURN_TCP`. `cargo test -p pod-turn --test
//! coturn_tcp -- --ignored` runs the test by hand.

mod common;

use common::coturn::{PASSWORD, REALM_NAME, USER, coturn_tcp, within};
use common::mock_tcp::echo_peer;
use common::tcp_settings;
use pod_turn::Client;

#[tokio::test]
#[ignore = "coturn with a TCP relay; scripts/ci/turn-tests.sh names it"]
async fn a_tcp_allocation_relays_a_framed_connection() {
    let Some(server) = coturn_tcp() else { return };
    let peer = echo_peer().await;
    let mut settings = tcp_settings(server, USER, PASSWORD);
    settings.realm = Some(REALM_NAME.into());
    let client = within(Client::allocate(settings)).await.expect("a TCP allocation");
    println!("relayed {} for {} s over TCP", client.relayed_address(), client.lifetime().as_secs());
    let mut stream = within(client.connect(peer)).await.expect("a Connect and a ConnectionBind");
    assert_eq!(stream.peer(), peer);
    within(stream.write_frame(b"coturn")).await.unwrap();
    let mut got = vec![0u8; b"echo:coturn".len()];
    within(tokio::io::AsyncReadExt::read_exact(&mut stream, &mut got)).await.unwrap();
    assert_eq!(got, b"echo:coturn");
    println!("coturn: an allocation over TCP relayed a TCP connection");
}
