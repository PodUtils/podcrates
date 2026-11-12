//! The STUN codec of `pod_proxy::stun` against coturn, a server that this
//! crate did not write (call C151, T-358): a Binding request from a UDP
//! socket on 127.0.0.1 gets a Binding success of the same transaction that
//! maps the address and the port of the socket, with a FINGERPRINT that
//! holds; a request with an attribute that coturn must understand and does
//! not know gets 420 with UNKNOWN-ATTRIBUTES.
//!
//! `scripts/ci/turn-tests.sh` starts coturn on 127.0.0.1 and sets
//! `POD_PROXY_TEST_STUN_SERVER` (`HOST:PORT`). With none, each test skips
//! with the reason, unless `POD_PROXY_TEST_STUN=required`, which the script
//! sets.

use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use pod_proxy::stun;

/// UNKNOWN-ATTRIBUTES (RFC 8489, 14.9).
const UNKNOWN_ATTRIBUTES: u16 = 0x000a;
/// A type of the range that a server must understand (0x0000 to 0x7FFF)
/// that no RFC assigns.
const NOT_ASSIGNED: u16 = 0x7f01;

/// The server of the variable, or `None` with a line when it names none.
/// `POD_PROXY_TEST_STUN=required` makes no server a failure.
fn server() -> Option<SocketAddr> {
    let Some(named) = std::env::var("POD_PROXY_TEST_STUN_SERVER").ok().filter(|v| !v.is_empty()) else {
        assert!(
            std::env::var("POD_PROXY_TEST_STUN").as_deref() != Ok("required"),
            "POD_PROXY_TEST_STUN=required: no STUN server here: POD_PROXY_TEST_STUN_SERVER names none"
        );
        println!("skipped: POD_PROXY_TEST_STUN_SERVER names no STUN server");
        return None;
    };
    Some(named.parse().expect("POD_PROXY_TEST_STUN_SERVER is IP:PORT"))
}

/// Send `request` to `server` from `socket` and give the answer of the
/// transaction `id`: three tries, as a datagram can be lost.
fn ask(socket: &UdpSocket, server: SocketAddr, request: &[u8], id: &stun::TransactionId) -> Vec<u8> {
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut buf = [0u8; 1500];
    for _ in 0..3 {
        socket.send_to(request, server).unwrap();
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            if from == server && n >= stun::HEADER && buf[8..stun::HEADER] == id[..] {
                return buf[..n].to_vec();
            }
        }
    }
    panic!("{server} did not answer 3 tries");
}

#[test]
fn a_binding_request_maps_the_address_of_its_socket() {
    let Some(server) = server() else { return };
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let id: stun::TransactionId = *b"pod-proxy-01";
    let answer = ask(&socket, server, &stun::binding_request(&id), &id);
    let read = stun::parse(&answer).unwrap();
    assert_eq!((read.kind, read.id), (stun::BINDING_SUCCESS, id));
    assert!(read.attribute(stun::XOR_MAPPED_ADDRESS).is_some(), "an answer of RFC 8489 maps by XOR");
    assert_eq!(read.mapped(), Some(Ok(socket.local_addr().unwrap())));
    assert!(stun::fingerprint_ok(&answer), "coturn runs with --fingerprint");
    println!("coturn: a Binding request mapped the address of its socket");
}

#[test]
fn a_binding_request_with_an_unknown_attribute_gets_420() {
    let Some(server) = server() else { return };
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let id: stun::TransactionId = *b"pod-proxy-02";
    let request = stun::message(stun::BINDING_REQUEST, &id, &[(NOT_ASSIGNED, b"abc".as_slice())]);
    let answer = ask(&socket, server, &request, &id);
    let read = stun::parse(&answer).unwrap();
    assert_eq!((read.kind, read.id), (stun::BINDING_ERROR, id));
    assert_eq!(read.error().unwrap().unwrap().0, 420);
    assert_eq!(read.attribute(UNKNOWN_ATTRIBUTES), Some(NOT_ASSIGNED.to_be_bytes().as_slice()));
    println!("coturn: a Binding request with an unknown attribute got 420");
}
