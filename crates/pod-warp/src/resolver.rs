//! The resolvers inside the tunnel (PodUtils/podnet issues 145 and 144):
//! DNS on UDP, or DNS over TLS (RFC 7858), each reached through the
//! session as any address. The proxy forwards DNS and resolves the names
//! of its clients through them ([`crate::proxy`]); the check of the data
//! path asks them ([`crate::check`]). Each function takes the session as a
//! [`Dial`], so a test can give any dial.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::{Entropy as _, OsEntropy};
use pod_core::{Class, Dial, Failure, Stage, Target};
use pod_dns::wire;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

/// The time limit of a DNS answer from one resolver.
pub const DNS_TIMEOUT: Duration = Duration::from_secs(5);
/// The interval of the resends of a question on UDP (PodUtils/podnet
/// issue 114: one lost datagram cost the full time limit).
pub const DNS_RESEND: Duration = Duration::from_millis(1200);
/// The time limit of a TCP connect through the tunnel.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The port of DNS on UDP, and of DNS over TLS.
pub const DNS_PORT: u16 = 53;
pub const DOT_PORT: u16 = 853;
/// The shortest DNS message: the header.
pub const DNS_HEADER: usize = 12;

fn usage(text: String) -> Failure {
    Failure::new(Stage::Dns, Class::Usage, text)
}

/// A resolver inside the tunnel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolver {
    Udp(SocketAddr),
    /// The address, and the name that the certificate must have (the
    /// address itself when no name is given).
    Tls {
        addr: SocketAddr,
        name: String,
    },
}

impl Resolver {
    /// `IP[:PORT]` (UDP, port 53, also `udp://`), or
    /// `tls://IP[:PORT][#NAME]` (port 853). An IPv6 address with a port is
    /// in brackets. A bad form is `Usage`.
    pub fn parse(spec: &str) -> Result<Resolver, Failure> {
        let spec = spec.trim();
        let (tls, rest) = match spec.strip_prefix("tls://") {
            Some(rest) => (true, rest),
            None => (false, spec.strip_prefix("udp://").unwrap_or(spec)),
        };
        let (addr, name) = match rest.split_once('#') {
            Some((a, n)) if tls && !n.is_empty() => (a, Some(n.to_string())),
            Some(_) => return Err(usage(format!("{spec:?}: a name (#NAME) is for tls:// only"))),
            None => (rest, None),
        };
        let port = if tls { DOT_PORT } else { DNS_PORT };
        let addr = match addr.parse::<SocketAddr>() {
            Ok(a) => a,
            Err(_) => {
                let ip = addr.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>();
                let ip = ip.map_err(|_| usage(format!("{spec:?} is not IP[:PORT] or tls://IP[:PORT][#NAME]")))?;
                SocketAddr::new(ip, port)
            }
        };
        Ok(match tls {
            true => Resolver::Tls { name: name.unwrap_or_else(|| addr.ip().to_string()), addr },
            false => Resolver::Udp(addr),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        match self {
            Resolver::Udp(a) | Resolver::Tls { addr: a, .. } => *a,
        }
    }
}

impl std::fmt::Display for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resolver::Udp(a) => write!(f, "{a}"),
            Resolver::Tls { addr, name } if *name == addr.ip().to_string() => write!(f, "tls://{addr}"),
            Resolver::Tls { addr, name } => write!(f, "tls://{addr}#{name}"),
        }
    }
}

/// A list of resolvers: each text is a comma-separated list of
/// [`Resolver::parse`] (podnet's `--resolver` and `PODNET_WARP_RESOLVERS`).
pub fn parse_resolvers(specs: &[String]) -> Result<Vec<Resolver>, Failure> {
    specs.iter().flat_map(|s| s.split(',')).filter(|s| !s.trim().is_empty()).map(Resolver::parse).collect()
}

/// The resolvers of a caller, with the TLS settings of DNS over TLS and
/// the time limits.
#[derive(Clone)]
pub struct Resolvers {
    /// In order: the first answer wins. Empty: the resolver of the tunnel
    /// on UDP ([`crate::service::tunnel_resolver`], 1.1.1.1:53).
    pub list: Vec<Resolver>,
    /// The TLS client of the caller (its provider and its roots), for a
    /// resolver over TLS; the crate has no roots of its own.
    pub tls: Option<Arc<rustls::ClientConfig>>,
    /// The time limit of an answer of one resolver ([`DNS_TIMEOUT`]).
    pub limit: Duration,
    /// The resend interval on UDP ([`DNS_RESEND`]).
    pub resend: Duration,
    /// The time limit of the TCP connect to a resolver over TLS
    /// ([`CONNECT_TIMEOUT`]).
    pub connect_timeout: Duration,
}

impl Default for Resolvers {
    fn default() -> Resolvers {
        Resolvers {
            list: Vec::new(),
            tls: None,
            limit: DNS_TIMEOUT,
            resend: DNS_RESEND,
            connect_timeout: CONNECT_TIMEOUT,
        }
    }
}

impl std::fmt::Debug for Resolvers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list: Vec<String> = self.list.iter().map(Resolver::to_string).collect();
        f.debug_struct("Resolvers")
            .field("list", &list)
            .field("tls", &self.tls.is_some())
            .field("limit", &self.limit)
            .field("resend", &self.resend)
            .finish()
    }
}

impl Resolvers {
    /// The resolvers `list`, with the default limits.
    pub fn new(list: Vec<Resolver>) -> Resolvers {
        Resolvers { list, ..Resolvers::default() }
    }

    /// The resolvers in use: [`Resolvers::list`], else the resolver of the
    /// tunnel.
    pub fn in_use(&self) -> Vec<Resolver> {
        match self.list.is_empty() {
            true => vec![Resolver::Udp(crate::service::tunnel_resolver())],
            false => self.list.clone(),
        }
    }

    /// One DNS message to each resolver in use, in turn, through `net`: the
    /// first answer with the id of `query`. No answer is `Unavailable`
    /// (stage `Dns`), with the cause of each resolver.
    pub async fn exchange(&self, net: &dyn Dial, query: &[u8]) -> Result<Vec<u8>, Failure> {
        let mut reasons = Vec::new();
        for resolver in self.in_use() {
            match self.exchange_with(net, &resolver, query).await {
                Ok(answer) => return Ok(answer),
                Err(why) => reasons.push(format!("{resolver}: {why}")),
            }
        }
        let text = format!("no DNS answer through WARP ({})", reasons.join("; "));
        Err(Failure::new(Stage::Dns, Class::Unavailable, text))
    }

    /// One DNS message to `resolver` through `net`.
    pub async fn exchange_with(&self, net: &dyn Dial, resolver: &Resolver, query: &[u8]) -> Result<Vec<u8>, String> {
        if query.len() < DNS_HEADER || query.len() > usize::from(u16::MAX) {
            return Err("not a DNS message".into());
        }
        match resolver {
            Resolver::Udp(addr) => udp_exchange(net, *addr, query, self.limit, self.resend).await,
            Resolver::Tls { addr, name } => {
                let Some(config) = &self.tls else {
                    return Err("DNS over TLS needs the TLS settings of the caller".into());
                };
                tls_exchange(net, *addr, name, config.clone(), query, self.connect_timeout, self.limit).await
            }
        }
    }

    /// The `kind` records of `name` (A or AAAA) from the resolvers in use.
    pub async fn query(&self, net: &dyn Dial, name: &str, kind: u16) -> Result<Vec<IpAddr>, Failure> {
        let message = question(name, kind).map_err(|e| usage(format!("{name}: {e}")))?;
        let answer = self.exchange(net, &message).await?;
        wire::addrs(&answer, name, kind).map_err(|e| {
            let class = if e.ends_with(pod_dns::NXDOMAIN) { Class::NotFound } else { Class::Unavailable };
            Failure::new(Stage::Dns, class, format!("{name} through WARP: {e}"))
        })
    }
}

/// A question of `kind` for `name` with a random id.
pub fn question(name: &str, kind: u16) -> Result<Vec<u8>, String> {
    let mut id = [0x5a, 0x5a];
    let _ = OsEntropy.fill(&mut id);
    let mut message = wire::query(name, kind)?;
    message[..2].copy_from_slice(&id);
    Ok(message)
}

/// One DNS message on UDP to `server` through `net`, sent again each
/// `resend` until an answer with its id comes from `server`, in at most
/// `limit`.
pub async fn udp_exchange(
    net: &dyn Dial,
    server: SocketAddr,
    query: &[u8],
    limit: Duration,
    resend: Duration,
) -> Result<Vec<u8>, String> {
    let socket = net.datagrams().await.map_err(|e| e.chain())?;
    let deadline = tokio::time::Instant::now() + limit;
    let mut buf = vec![0u8; 4096];
    loop {
        pod_core::datagrams::send_to(socket.as_ref(), query, server).await.map_err(|e| e.to_string())?;
        let again = (tokio::time::Instant::now() + resend).min(deadline);
        loop {
            match tokio::time::timeout_at(again, pod_core::datagrams::recv_from(socket.as_ref(), &mut buf)).await {
                Ok(Ok((n, from))) if from == server && n >= 2 && buf[..2] == query[..2] => {
                    return Ok(buf[..n].to_vec());
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => break,
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("no answer in {} ms", limit.as_millis()));
        }
    }
}

/// One DNS message over TLS to `addr` through `net`: an address, so no name
/// to resolve; the certificate must name `name`.
async fn tls_exchange(
    net: &dyn Dial,
    addr: SocketAddr,
    name: &str,
    config: Arc<rustls::ClientConfig>,
    query: &[u8],
    connect_timeout: Duration,
    limit: Duration,
) -> Result<Vec<u8>, String> {
    let deadline = tokio::time::Instant::now() + connect_timeout;
    let stream = net.dial(&Target::Addr(addr), deadline).await.map_err(|e| e.chain())?.io;
    let server =
        rustls_pki_types::ServerName::try_from(name.to_string()).map_err(|_| format!("{name:?} is not a TLS name"))?;
    let connect = tokio_rustls::TlsConnector::from(config).connect(server, stream);
    let tls = tokio::time::timeout(limit, connect)
        .await
        .map_err(|_| format!("no TLS with {addr} in {} s", limit.as_secs()))?
        .map_err(|e| format!("TLS with {addr}: {e}"))?;
    tokio::time::timeout(limit, dot_exchange(tls, query))
        .await
        .map_err(|_| format!("no DNS over TLS answer from {addr}"))?
        .map_err(|e| e.to_string())
}

/// One DNS message on a stream of RFC 7858 (a length of 2 bytes before
/// each message); the answer with the id of the query. An answer with
/// another id is skipped.
pub async fn dot_exchange<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, query: &[u8]) -> std::io::Result<Vec<u8>> {
    let len = u16::try_from(query.len()).ok().filter(|_| query.len() >= DNS_HEADER);
    let len = len.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a DNS message"))?;
    let mut framed = len.to_be_bytes().to_vec();
    framed.extend_from_slice(query);
    stream.write_all(&framed).await?;
    stream.flush().await?;
    loop {
        let mut len = [0u8; 2];
        stream.read_exact(&mut len).await?;
        let mut answer = vec![0u8; usize::from(u16::from_be_bytes(len))];
        stream.read_exact(&mut answer).await?;
        if answer.get(..2) == query.get(..2) {
            return Ok(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PodUtils/podnet issues 145 and 144: the forms of `--resolver`.
    #[test]
    fn the_resolver_specs() {
        let p = |s: &str| Resolver::parse(s).unwrap();
        assert_eq!(p("9.9.9.9"), Resolver::Udp("9.9.9.9:53".parse().unwrap()));
        assert_eq!(p("9.9.9.9:5353"), Resolver::Udp("9.9.9.9:5353".parse().unwrap()));
        assert_eq!(p("udp://9.9.9.9"), Resolver::Udp("9.9.9.9:53".parse().unwrap()));
        assert_eq!(p("2620:fe::fe"), Resolver::Udp("[2620:fe::fe]:53".parse().unwrap()));
        assert_eq!(
            p("tls://1.1.1.1#cloudflare-dns.com"),
            Resolver::Tls { addr: "1.1.1.1:853".parse().unwrap(), name: "cloudflare-dns.com".into() }
        );
        assert_eq!(
            p("tls://[2606:4700:4700::1111]:853"),
            Resolver::Tls { addr: "[2606:4700:4700::1111]:853".parse().unwrap(), name: "2606:4700:4700::1111".into() }
        );
        assert_eq!(p("tls://9.9.9.9").to_string(), "tls://9.9.9.9:853");
        assert_eq!(p("tls://1.1.1.1#one.one").to_string(), "tls://1.1.1.1:853#one.one");
        for bad in ["dns.example", "9.9.9.9#name", "tls://", "1.2.3"] {
            assert_eq!(Resolver::parse(bad).unwrap_err().class, Class::Usage, "{bad}");
        }
        let list = parse_resolvers(&["9.9.9.9,tls://1.1.1.1".into(), String::new()]).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(Resolvers::default().in_use(), vec![Resolver::Udp("1.1.1.1:53".parse().unwrap())]);
    }

    /// PodUtils/podnet issue 144: the framing of RFC 7858 (a length of 2
    /// bytes); an answer with another id is skipped.
    #[tokio::test]
    async fn dns_over_tls_frames_each_message() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut query = wire::query("example.com", wire::A).unwrap();
        query[..2].copy_from_slice(&[0x13, 0x57]);
        let q = query.clone();
        let peer = tokio::spawn(async move {
            let mut len = [0u8; 2];
            server.read_exact(&mut len).await.unwrap();
            let mut msg = vec![0u8; usize::from(u16::from_be_bytes(len))];
            server.read_exact(&mut msg).await.unwrap();
            assert_eq!(msg, q);
            for id in [[0x99, 0x99], [0x13, 0x57]] {
                let mut answer = msg.clone();
                answer[..2].copy_from_slice(&id);
                let mut framed = (answer.len() as u16).to_be_bytes().to_vec();
                framed.extend_from_slice(&answer);
                server.write_all(&framed).await.unwrap();
            }
        });
        let answer = dot_exchange(client, &query).await.unwrap();
        assert_eq!(&answer[..2], &[0x13, 0x57]);
        peer.await.unwrap();
        let (client, _server) = tokio::io::duplex(64);
        assert!(dot_exchange(client, b"short").await.is_err());
    }
}
