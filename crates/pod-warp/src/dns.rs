//! Names inside the tunnel: DNS over TCP (RFC 7766, a length of two bytes
//! before each message) to Cloudflare's resolver, reached through WARP as
//! the target is, in the wire form of `pod-dns`. The host of a cage
//! resolves no name itself, and a proxy would see each name, so the
//! tunnel's own way out answers.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_dns::wire;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::net::WarpNet;

/// Cloudflare's resolvers, the first of each family first; the tunnel
/// reaches them as any address.
pub const RESOLVERS: [IpAddr; 4] = [
    IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
    IpAddr::V4(Ipv4Addr::new(1, 0, 0, 1)),
    IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)),
    IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1001)),
];

/// The DNS port.
pub const PORT: u16 = 53;

/// The addresses of `name`, A then AAAA, from the first resolver of
/// `resolvers` that the device can reach and that answers; NXDOMAIN ends
/// the lookup. An address in the name is the address.
pub async fn lookup(net: &WarpNet, resolvers: &[IpAddr], name: &str, limit: Duration) -> Result<Vec<IpAddr>, Failure> {
    let bare = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')).unwrap_or(name);
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let families: Vec<bool> = net.addresses().iter().map(IpAddr::is_ipv4).collect();
    let mut reasons = Vec::new();
    for resolver in resolvers.iter().filter(|r| families.contains(&r.is_ipv4())) {
        let mut found = Vec::new();
        let mut answered = false;
        for kind in [wire::A, wire::AAAA] {
            match ask(net, SocketAddr::new(*resolver, PORT), name, kind, limit).await {
                Ok(ips) => {
                    answered = true;
                    found.extend(ips);
                }
                Err(e) if e.ends_with(pod_dns::NXDOMAIN) => {
                    return Err(Failure::new(Stage::Dns, Class::NotFound, format!("{name}: {resolver}: {e}")));
                }
                Err(e) => reasons.push(format!("{resolver}: {e}")),
            }
        }
        if answered && !found.is_empty() {
            return Ok(found);
        }
        if answered {
            reasons.push(format!("{resolver}: no address"));
        }
    }
    let why = if reasons.is_empty() { "no resolver of the families of this device".into() } else { reasons.join("; ") };
    Err(Failure::new(Stage::Dns, Class::Unavailable, format!("{name} inside WARP: {why}")))
}

/// One question to `resolver`, on a connection of its own.
async fn ask(
    net: &WarpNet,
    resolver: SocketAddr,
    name: &str,
    kind: u16,
    limit: Duration,
) -> Result<Vec<IpAddr>, String> {
    let query = wire::query(name, kind)?;
    let deadline = tokio::time::Instant::now() + limit;
    let mut stream = net.connect_addr(resolver, deadline).await.map_err(|e| e.chain())?.io;
    let asked = async {
        let len = u16::try_from(query.len()).map_err(|_| std::io::Error::other("the query is too long"))?;
        let mut framed = len.to_be_bytes().to_vec();
        framed.extend_from_slice(&query);
        stream.write_all(&framed).await?;
        let mut len = [0u8; 2];
        stream.read_exact(&mut len).await?;
        let mut answer = vec![0u8; usize::from(u16::from_be_bytes(len))];
        stream.read_exact(&mut answer).await?;
        Ok::<_, std::io::Error>(answer)
    };
    let answer = tokio::time::timeout_at(deadline, asked)
        .await
        .map_err(|_| format!("no answer within {} s", limit.as_secs()))?
        .map_err(|e| e.to_string())?;
    wire::addrs(&answer, name, kind)
}
