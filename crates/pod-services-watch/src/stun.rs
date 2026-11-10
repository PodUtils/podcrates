//! `stun-binding`: a STUN server answers a Binding request (RFC 8489) with
//! a Binding success that maps an address. UDP goes direct: a proxy of the
//! runner carries TCP alone.

use std::net::SocketAddr;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_proxy::stun;
use pod_services::{Service, Watch};
use tokio::net::UdpSocket;

use crate::args::Args;
use crate::{Checked, Finding, Net, Outcome};

/// The tries of one request: a datagram can be lost.
const TRIES: u32 = 3;
/// The wait for an answer to one try.
const WAIT: Duration = Duration::from_secs(2);

/// `stun-binding`: the endpoint answers a Binding request with a Binding
/// success of the same transaction that holds a mapped address.
pub async fn binding(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let endpoint = Args::new(service, watch).endpoint()?;
    let mut addrs = endpoint.socket_addrs();
    if addrs.is_empty() {
        let authority = (endpoint.host().trim_end_matches('.'), endpoint.port());
        let found = tokio::time::timeout(net.limit, tokio::net::lookup_host(authority)).await;
        addrs = match found {
            Ok(Ok(found)) => found.collect(),
            _ => return Err(Finding::failed(format!("{} has no address", endpoint.host()))),
        };
    }
    // An IPv4 address first: a runner has IPv4 more often than IPv6.
    addrs.sort_by_key(|a| !a.is_ipv4());
    let mut last = Finding::failed(format!("{} has no address", endpoint.host()));
    for addr in addrs {
        match ask(addr).await {
            Ok(mapped) => {
                let mut out = Outcome::default();
                out.saw(format!("{} ({addr}) mapped this host to {mapped}", endpoint.authority()));
                return Ok(out);
            }
            Err(finding) => last = finding,
        }
    }
    Err(last)
}

/// The mapped address that the server at `addr` gives.
async fn ask(addr: SocketAddr) -> Result<SocketAddr, Finding> {
    let local: SocketAddr = if addr.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { ([0u16; 8], 0).into() };
    let socket = UdpSocket::bind(local).await.map_err(|e| Finding::failed(format!("a UDP socket: {e}")))?;
    let mut id = [0u8; 12];
    OsEntropy.fill(&mut id).map_err(|e| Finding::failed(e.to_string()))?;
    let request = stun::binding_request(&id);
    let mut buf = [0u8; 1500];
    for _ in 0..TRIES {
        socket.send_to(&request, addr).await.map_err(|e| Finding::failed(format!("{addr}: {e}")))?;
        let Ok(got) = tokio::time::timeout(WAIT, socket.recv_from(&mut buf)).await else { continue };
        let (n, from) = got.map_err(|e| Finding::failed(format!("{addr}: {e}")))?;
        if from != addr {
            continue;
        }
        let message = stun::parse(&buf[..n]).map_err(|e| Finding::failed(format!("{addr} answered no STUN: {e}")))?;
        if message.id != id {
            continue;
        }
        if message.kind != stun::BINDING_SUCCESS {
            return Err(Finding::failed(format!("{addr} answered the message type 0x{:04x}", message.kind)));
        }
        let mapped = message.attributes.iter().find_map(|(kind, value)| match *kind {
            stun::XOR_MAPPED_ADDRESS => stun::read_address(value, Some(&id)).ok(),
            stun::MAPPED_ADDRESS => stun::read_address(value, None).ok(),
            _ => None,
        });
        return mapped.ok_or_else(|| Finding::failed(format!("the answer of {addr} maps no address")));
    }
    Err(Finding::failed(format!("{addr} did not answer {TRIES} Binding requests")))
}
