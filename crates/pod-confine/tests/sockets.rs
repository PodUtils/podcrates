//! The sockets that Landlock does not check, in a real child: under a TCP
//! list a packet socket, a raw socket of IP, an SCTP socket and a socket of
//! another family fail with the answer of a kernel with none; TCP, UDP,
//! Unix and netlink sockets still open. The calls use IPv4: a host can
//! have no IPv6. A host with no Landlock skips.
#![cfg(target_os = "linux")]

mod child;

use child::{code, in_child};
use pod_confine::{NET_ABI, Spec, apply};
use pod_hostprobe::sys as k;

const EPROTONOSUPPORT: i64 = -93;
const ESOCKTNOSUPPORT: i64 = -94;
const EAFNOSUPPORT: i64 = -97;

/// `AF_VSOCK`: a socket to the host of a guest.
const AF_VSOCK: u64 = 40;

#[test]
fn a_tcp_list_closes_the_sockets_that_landlock_does_not_check() {
    let abi = match pod_confine::check() {
        Ok(abi) if abi >= NET_ABI => abi,
        Ok(abi) => return pod_testkit::host::skip_line(&format!("Landlock ABI {abi} has no TCP rules")),
        Err(e) => return pod_testkit::host::skip_line(&e.to_string()),
    };
    let compiled = Spec::new().connect_to(443).compile(abi).unwrap();
    let calls = || {
        let socket =
            |family: u64, kind: u64, protocol: u64| code(k::socket(family, kind | k::SOCK_CLOEXEC, protocol)).min(0);
        [
            socket(k::AF_PACKET, k::SOCK_RAW, 0),
            socket(AF_VSOCK, k::SOCK_STREAM, 0),
            socket(k::AF_INET, k::SOCK_RAW, 1),
            socket(k::AF_INET, k::SOCK_SEQPACKET, 0),
            socket(k::AF_INET, k::SOCK_STREAM, k::IPPROTO_SCTP),
            socket(k::AF_INET, k::SOCK_STREAM, k::IPPROTO_TCP),
            socket(k::AF_INET, k::SOCK_DGRAM, 0),
            socket(k::AF_UNIX, k::SOCK_STREAM, 0),
            socket(k::AF_NETLINK, k::SOCK_RAW, 0),
        ]
    };
    let free = in_child(calls);
    assert_eq!(free[5..], [0, 0, 0, 0], "TCP, UDP, Unix and netlink with no confinement: {free:?}");
    let got = in_child(|| {
        if let Err(r) = apply(&compiled) {
            return [-1000 - i64::from(r.call.code()); 9];
        }
        calls()
    });
    assert_eq!(
        got,
        [EAFNOSUPPORT, EAFNOSUPPORT, ESOCKTNOSUPPORT, ESOCKTNOSUPPORT, EPROTONOSUPPORT, 0, 0, 0, 0],
        "packet, VSOCK, raw IP, SCTP over SOCK_SEQPACKET, SCTP stream, TCP, UDP, Unix, netlink (free: {free:?})"
    );
}
