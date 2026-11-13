//! The exit of a WARP session (PodUtils/podnet issue 147): the colo, the
//! country and the address that a destination sees, from `/cdn-cgi/trace`
//! of 1.1.1.1 through the tunnel, over HTTPS (plain HTTP on port 80 gave no
//! trace in podnet's live run 41599321447). The request goes to an
//! address, so no DNS step comes first. The certificate must have the name
//! `one.one.one.one`; the TLS client (its provider and its roots) is the
//! caller's, as the crate has no roots.
//!
//! The IPv6 address of the exit (PodUtils/podnet issue 184) is the trace
//! over IPv6, else the TXT record of `whoami.cloudflare` in class CH at the
//! IPv6 resolver of Cloudflare.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_core::{Class, Dial, Failure, Stage, Target};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// The time limit of the trace.
pub const TRACE_TIMEOUT: Duration = Duration::from_secs(15);
/// The most bytes of the answer.
const MAX_ANSWER: u64 = 16 * 1024;
/// The ALPN of the trace: HTTP/1.1 (the request is HTTP/1.0).
const ALPN_HTTP1: &[u8] = b"http/1.1";

/// The service `cloudflare-dns` of `pod-services`.
fn dns() -> &'static pod_services::Service {
    pod_services::get("cloudflare-dns").expect("pod-services holds the service cloudflare-dns")
}

/// The trace of 1.1.1.1: the endpoint `trace` of `cloudflare-dns`.
fn trace_endpoint() -> &'static pod_services::Endpoint {
    dns().endpoint("trace").expect("cloudflare-dns of pod-services has the endpoint trace")
}

/// The address of the trace of the family `v6`.
fn trace_address_of(v6: bool) -> IpAddr {
    let found = trace_endpoint().ip_addrs().into_iter().find(|ip| ip.is_ipv6() == v6);
    found.expect("the trace of pod-services has an address of each family")
}

/// The IPv4 address of the trace.
pub fn trace_address() -> IpAddr {
    trace_address_of(false)
}

/// The IPv6 address of the trace and of the whoami query.
pub fn trace_address_v6() -> IpAddr {
    trace_address_of(true)
}

/// The name of the trace (the certificate and the Host header).
pub fn trace_name() -> &'static str {
    trace_endpoint().host()
}

fn trace_path() -> &'static str {
    trace_endpoint().path().unwrap_or("/cdn-cgi/trace")
}

/// The name of the whoami query: the fact `whoami-name`.
fn whoami_name() -> &'static str {
    dns().text("whoami-name").expect("cloudflare-dns of pod-services holds the fact whoami-name")
}

/// The exit of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    /// The data center (IATA code), as `AMS`.
    pub colo: String,
    /// The country (ISO 3166), as `NL`.
    pub loc: Option<String>,
    /// The address that the destination saw.
    pub ip: Option<String>,
    /// `on` or `plus` through WARP.
    pub warp: Option<String>,
    /// The time of the request.
    pub rtt: Duration,
    /// The IPv6 address of the exit, when it was asked for.
    pub ipv6: Option<String>,
    /// Why the IPv6 address of the exit is not known.
    pub ipv6_error: Option<String>,
}

impl Exit {
    /// podnet's JSON form of `warp status`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "colo": self.colo,
            "loc": self.loc,
            "ip": self.ip,
            "warp": self.warp,
            "rtt_ms": self.rtt.as_millis() as u64,
            "ipv6": self.ipv6,
            "ipv6_error": self.ipv6_error,
        })
    }
}

/// The exit of the body of a trace (`key=value` lines). `None` with no colo
/// of 3 capital letters.
pub fn parse(body: &str, rtt: Duration) -> Option<Exit> {
    let get = |key: &str| {
        body.lines().find_map(|l| {
            l.strip_prefix(key)
                .and_then(|r| r.strip_prefix('='))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
    };
    let colo = get("colo")?;
    if colo.len() != 3 || !colo.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    Some(Exit { colo, loc: get("loc"), ip: get("ip"), warp: get("warp"), rtt, ipv6: None, ipv6_error: None })
}

/// The exit of the session `net`, with the TLS client `tls` of the caller.
pub async fn trace(net: &dyn Dial, tls: &Arc<rustls::ClientConfig>) -> Result<Exit, Failure> {
    trace_at(net, tls, trace_address()).await
}

/// The IPv6 address of the exit of the session `net`: the trace over IPv6,
/// else the TXT record of whoami in class CH at the IPv6 resolver of
/// Cloudflare.
pub async fn egress_v6(net: &dyn Dial, tls: &Arc<rustls::ClientConfig>) -> Result<String, Failure> {
    let mut errors = Vec::new();
    match trace_at(net, tls, trace_address_v6()).await {
        Ok(exit) => match exit.ip {
            Some(ip) if ip.contains(':') => return Ok(ip),
            other => errors.push(format!("the IPv6 trace gave {}", other.as_deref().unwrap_or("no ip"))),
        },
        Err(err) => errors.push(format!("the IPv6 trace: {}", err.chain())),
    }
    match whoami_v6(net).await {
        Ok(ip) => Ok(ip),
        Err(err) => {
            let text = format!("{}; {}: {}", errors.join("; "), whoami_name(), err.chain());
            Err(Failure::new(Stage::Warp, err.class, text))
        }
    }
}

/// The DNS query of the TXT record of `whoami.cloudflare` in class CH.
pub fn whoami_query(id: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(40);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in whoami_name().split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&16u16.to_be_bytes()); // TXT
    q.extend_from_slice(&3u16.to_be_bytes()); // CH
    q
}

/// The text of the first TXT answer of a DNS message.
pub fn first_txt(message: &[u8]) -> Option<String> {
    let u16_at = |i: usize| Some(u16::from_be_bytes([*message.get(i)?, *message.get(i + 1)?]));
    let skip_name = |mut i: usize| -> Option<usize> {
        loop {
            let len = *message.get(i)? as usize;
            if len == 0 {
                return Some(i + 1);
            }
            if len & 0xc0 == 0xc0 {
                return Some(i + 2);
            }
            i += 1 + len;
        }
    };
    let questions = u16_at(4)?;
    let answers = u16_at(6)?;
    let mut i = 12;
    for _ in 0..questions {
        i = skip_name(i)? + 4;
    }
    for _ in 0..answers {
        i = skip_name(i)?;
        let kind = u16_at(i)?;
        let rdlen = u16_at(i + 8)? as usize;
        let rdata = message.get(i + 10..i + 10 + rdlen)?;
        if kind == 16 {
            let mut text = String::new();
            let mut j = 0;
            while j < rdata.len() {
                let n = rdata[j] as usize;
                text.push_str(&String::from_utf8_lossy(rdata.get(j + 1..j + 1 + n)?));
                j += 1 + n;
            }
            return Some(text);
        }
        i += 10 + rdlen;
    }
    None
}

async fn whoami_v6(net: &dyn Dial) -> Result<String, Failure> {
    use crate::resolver::{DNS_RESEND, DNS_TIMEOUT, udp_exchange};
    let server = SocketAddr::new(trace_address_v6(), 53);
    let query = whoami_query(0x7e57);
    let answer = udp_exchange(net, server, &query, DNS_TIMEOUT, DNS_RESEND)
        .await
        .map_err(|why| Failure::new(Stage::Dns, Class::Unavailable, format!("{server} through WARP: {why}")))?;
    let text =
        first_txt(&answer).ok_or_else(|| Failure::new(Stage::Dns, Class::Protocol, "the answer has no TXT record"))?;
    if !text.contains(':') {
        let text = format!("the answer is not an IPv6 address: {text}");
        return Err(Failure::new(Stage::Dns, Class::Protocol, text));
    }
    Ok(text)
}

/// The trace at `address` through `net`.
pub async fn trace_at(net: &dyn Dial, tls: &Arc<rustls::ClientConfig>, address: IpAddr) -> Result<Exit, Failure> {
    let start = Instant::now();
    let deadline = tokio::time::Instant::now() + TRACE_TIMEOUT;
    let work = async {
        let stream = net.dial(&Target::Addr(SocketAddr::new(address, 443)), deadline).await?.io;
        let mut config = (**tls).clone();
        config.alpn_protocols = vec![ALPN_HTTP1.to_vec()];
        let name = rustls_pki_types::ServerName::try_from(trace_name()).expect("the trace has a TLS name");
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, stream)
            .await
            .map_err(|e| Failure::caused(Stage::Tls, Class::of_io(e.kind()), format!("TLS with {address}"), e))?;
        // HTTP/1.0: the body is not chunked, and ends with the connection.
        let request =
            format!("GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: pod-warp\r\n\r\n", trace_path(), trace_name());
        let io = |e: std::io::Error| Failure::io(Stage::Warp, e);
        tls.write_all(request.as_bytes()).await.map_err(io)?;
        tls.flush().await.map_err(io)?;
        let mut answer = Vec::new();
        // A server that closes with no close_notify is an error of rustls;
        // the bytes before it count.
        let _ = (&mut tls).take(MAX_ANSWER).read_to_end(&mut answer).await;
        Ok::<_, Failure>(answer)
    };
    let answer = tokio::time::timeout_at(deadline, work).await.map_err(|_| {
        let text = format!("no trace from {address} through WARP in {} s", TRACE_TIMEOUT.as_secs());
        Failure::new(Stage::Warp, Class::Timeout, text)
    })??;
    let text = String::from_utf8_lossy(&answer);
    let status = text.lines().next().unwrap_or("").trim().to_string();
    let Some((_, body)) = text.split_once("\r\n\r\n") else {
        let text = format!("the trace answer has no body ({status:?})");
        return Err(Failure::new(Stage::Warp, Class::Protocol, text));
    };
    parse(body, start.elapsed()).ok_or_else(|| {
        let text = format!("the trace answer has no colo ({status:?}, {} bytes)", answer.len());
        Failure::new(Stage::Warp, Class::Protocol, text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // PodUtils/podnet issue 184: the whoami query and the TXT of its answer.
    #[test]
    fn the_whoami_query_and_its_answer() {
        let q = whoami_query(0x1234);
        assert_eq!(&q[..2], &[0x12, 0x34]);
        assert_eq!(&q[q.len() - 4..], &[0, 16, 0, 3]);
        // The answer: the question, then a TXT record with a pointer name.
        let mut a = q.clone();
        a[2] = 0x81;
        a[3] = 0x80;
        a[7] = 1; // one answer
        let ip = "2a09:bac1:28a0:8::1f:2a";
        a.extend_from_slice(&[0xc0, 0x0c, 0, 16, 0, 3, 0, 0, 0, 0]);
        a.extend_from_slice(&((ip.len() + 1) as u16).to_be_bytes());
        a.push(ip.len() as u8);
        a.extend_from_slice(ip.as_bytes());
        assert_eq!(first_txt(&a).as_deref(), Some(ip));
        assert_eq!(first_txt(&q), None);
    }

    // PodUtils/podnet issue 184: the JSON keeps ip and adds ipv6.
    #[test]
    fn the_json_keeps_ip_and_adds_ipv6() {
        let mut exit = parse("colo=AMS\nip=104.28.0.1\n", Duration::from_millis(5)).unwrap();
        exit.ipv6 = Some("2a09:bac1::1".into());
        let doc = exit.to_json();
        assert_eq!(doc["ip"], "104.28.0.1");
        assert_eq!(doc["ipv6"], "2a09:bac1::1");
        assert!(doc["ipv6_error"].is_null());
    }

    #[test]
    fn the_trace_body_gives_the_exit() {
        let body = "fl=12f\nh=1.1.1.1\nip=2a09:bac1::7\nts=1.2\nvisit_scheme=http\ncolo=AMS\nsliver=none\nhttp=http/1.1\nloc=NL\ntls=off\nsni=off\nwarp=on\ngateway=off\n";
        let exit = parse(body, Duration::from_millis(42)).unwrap();
        assert_eq!(exit.colo, "AMS");
        assert_eq!(exit.loc.as_deref(), Some("NL"));
        assert_eq!(exit.warp.as_deref(), Some("on"));
        assert_eq!(exit.ip.as_deref(), Some("2a09:bac1::7"));
        assert_eq!(exit.to_json()["rtt_ms"], 42);
        assert!(parse("loc=NL\n", Duration::ZERO).is_none());
        assert!(parse("colo=amsterdam\n", Duration::ZERO).is_none());
        assert!(parse("colo=\n", Duration::ZERO).is_none());
    }

    /// The facts of the trace are those of `pod-services`.
    #[test]
    fn the_trace_is_the_endpoint_of_the_services() {
        assert_eq!(trace_address(), "1.1.1.1".parse::<IpAddr>().unwrap());
        assert!(trace_address_v6().is_ipv6());
        assert_eq!(trace_name(), "one.one.one.one");
        assert_eq!(trace_path(), "/cdn-cgi/trace");
    }
}
