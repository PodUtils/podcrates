//! `proxy-banner`: a session through the share link of a public proxy node
//! reaches the SSH server of another service and gives its banner. The
//! watch speaks Trojan and VLESS over TCP, with or without TLS; another
//! transport of a link is reported as not checked.

use pod_core::BoxedIo;
use pod_services::{Service, Watch};
use sha2::{Digest, Sha224};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::args::{Args, hex};
use crate::ws::{banner_line, ssh_endpoint};
use crate::{Checked, Finding, Net, Outcome};

/// A share link, as the public lists write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub scheme: String,
    /// The password of Trojan, the UUID of VLESS.
    pub user: String,
    pub host: String,
    pub port: u16,
    /// The parameters of the query, in order.
    pub params: Vec<(String, String)>,
}

impl Link {
    /// A link `SCHEME://USER@HOST:PORT?QUERY#NAME`.
    pub fn parse(text: &str) -> Result<Link, String> {
        let (scheme, rest) = text.split_once("://").ok_or("a share link has no scheme")?;
        let rest = rest.split('#').next().unwrap_or_default();
        let (main, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (user, authority) = main.rsplit_once('@').ok_or("a share link has no user")?;
        let authority = authority.trim_end_matches('/');
        let (host, port) = authority.rsplit_once(':').ok_or("a share link has no port")?;
        let port = port.parse().map_err(|_| format!("the port {port:?} of a share link"))?;
        let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        let params = query
            .split('&')
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
            .collect();
        Ok(Link { scheme: scheme.to_ascii_lowercase(), user: user.to_string(), host, port, params })
    }

    /// The value of the parameter `name`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `proxy-banner`: through the link of `fact`, the endpoint of the service
/// `target` sends its SSH banner.
pub async fn banner(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let text = args.fact("fact")?.value().as_text().ok_or_else(|| Finding::failed("the link is no text"))?;
    let link = Link::parse(text).map_err(Finding::failed)?;
    let target = ssh_endpoint(args.target("target", &net.services)?)?;
    let (host, port) = (target.host().trim_end_matches('.').to_string(), target.port());
    let transport = link.param("type").unwrap_or("tcp");
    let security = link.param("security").unwrap_or("none");
    let mut out = Outcome::default();
    if !matches!(security, "tls" | "none") || transport != "tcp" {
        out.skip(format!("the watch speaks no {} transport {transport} with security {security}", link.scheme));
        return Ok(out);
    }
    let line = tokio::time::timeout(net.limit * 2, async {
        let io = open(net, &link, security).await?;
        match link.scheme.as_str() {
            "trojan" => trojan(io, &link, &host, port).await,
            "vless" => vless(io, &link, &host, port).await,
            scheme => Err(Finding::failed(format!("the watch speaks no {scheme} link"))),
        }
    })
    .await
    .map_err(|_| {
        Finding::failed(format!("no banner through {}:{} within {:?}", link.host, link.port, net.limit * 2))
    })??;
    out.saw(format!("{} {}:{} to {host}:{port} gave {line:?}", link.scheme, link.host, link.port));
    Ok(out)
}

/// The stream to the node: TCP, and TLS for its SNI when the link asks.
async fn open(net: &Net, link: &Link, security: &str) -> Result<BoxedIo, Finding> {
    if security == "tls" {
        let sni = link.param("sni").filter(|s| !s.is_empty()).unwrap_or(&link.host);
        return Ok(Box::new(net.tls(&link.host, link.port, sni).await?));
    }
    net.tcp(&link.host, link.port).await
}

/// The address of a request of SOCKS5's form: a name, its length first.
fn address(out: &mut Vec<u8>, kind: u8, host: &str, port: u16, port_first: bool) {
    if port_first {
        out.extend_from_slice(&port.to_be_bytes());
    }
    out.push(kind);
    out.push(host.len() as u8);
    out.extend_from_slice(host.as_bytes());
    if !port_first {
        out.extend_from_slice(&port.to_be_bytes());
    }
}

/// Trojan: the SHA-224 of the password in hex, CRLF, a CONNECT to
/// `host:port`, CRLF; then the bytes of the target.
async fn trojan(mut io: BoxedIo, link: &Link, host: &str, port: u16) -> Result<String, Finding> {
    let mut request = hex(&Sha224::digest(link.user.as_bytes())).into_bytes();
    request.extend_from_slice(b"\r\n\x01");
    address(&mut request, 0x03, host, port, false);
    request.extend_from_slice(b"\r\n");
    io.write_all(&request).await.map_err(|e| Finding::failed(format!("the Trojan request: {e}")))?;
    read_line(&mut io, Vec::new()).await
}

/// VLESS: version 0, the UUID, no addon, a CONNECT of TCP to `host:port`;
/// the answer starts with version 0 and the length of its addons.
async fn vless(mut io: BoxedIo, link: &Link, host: &str, port: u16) -> Result<String, Finding> {
    let uuid: String = link.user.chars().filter(|c| *c != '-').collect();
    if uuid.len() != 32 {
        return Err(Finding::failed("the user of a VLESS link is no UUID"));
    }
    let mut request = vec![0u8];
    for i in (0..32).step_by(2) {
        request.push(u8::from_str_radix(&uuid[i..i + 2], 16).map_err(|_| Finding::failed("the UUID is not hex"))?);
    }
    request.extend_from_slice(&[0, 0x01]);
    address(&mut request, 0x02, host, port, true);
    io.write_all(&request).await.map_err(|e| Finding::failed(format!("the VLESS request: {e}")))?;
    let mut head = [0u8; 2];
    io.read_exact(&mut head).await.map_err(|e| Finding::failed(format!("the VLESS answer: {e}")))?;
    let mut addons = vec![0u8; usize::from(head[1])];
    io.read_exact(&mut addons).await.map_err(|e| Finding::failed(format!("the VLESS answer: {e}")))?;
    read_line(&mut io, Vec::new()).await
}

/// The banner line of the target from `io`, after the bytes of `bytes`.
async fn read_line<S: AsyncRead + AsyncWrite + Unpin>(io: &mut S, mut bytes: Vec<u8>) -> Result<String, Finding> {
    let mut chunk = [0u8; 512];
    loop {
        if let Some(line) = banner_line(&bytes)? {
            return Ok(line);
        }
        let n = match io.read(&mut chunk).await {
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => 0,
            other => other.map_err(|e| Finding::failed(format!("the session: {e}")))?,
        };
        if n == 0 {
            return Err(Finding::failed("the node closed before a banner"));
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
}
