//! `proxy-banner`: a session through the share link of a public proxy node
//! reaches the SSH server of another service and gives its banner. The
//! watch speaks Trojan and VLESS over TCP, and VLESS over XHTTP in the mode
//! `packet-up` (`xhttp.rs`), with or without TLS; another transport,
//! security, mode or VLESS form of a link is reported as not checked.

use pod_core::BoxedIo;
use pod_services::{Service, Watch};
use sha2::{Digest, Sha224};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::args::{Args, hex};
use crate::ws::{banner_line, ssh_endpoint};
use crate::xhttp::{self, Plan};
use crate::{Checked, Finding, Net, Outcome};

/// A share link, as the public lists write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub scheme: String,
    /// The password of Trojan, the UUID of VLESS; percent-decoded.
    pub user: String,
    pub host: String,
    pub port: u16,
    /// The parameters of the query, in order; each value percent-decoded.
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
            .map(|(k, v)| (k.to_ascii_lowercase(), decoded(v)))
            .collect();
        Ok(Link { scheme: scheme.to_ascii_lowercase(), user: decoded(user), host, port, params })
    }

    /// The value of the parameter `name`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `text` with each `%XX` decoded: a link writes `/` as `%2F` in a path, and
/// a password with `@` as `%40`. A `%` with no two hex digits stays.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let digit = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], digit(i + 1), digit(i + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `proxy-banner`: through the link of `fact`, the endpoint of the service
/// `target` sends its SSH banner.
pub async fn banner(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let text = args.fact("fact")?.value().as_text().ok_or_else(|| Finding::failed("the link is no text"))?;
    let link = Link::parse(text).map_err(Finding::failed)?;
    let target = ssh_endpoint(args.target("target", &net.services)?)?;
    let (host, port) = (target.host().trim_end_matches('.').to_string(), target.port());
    // Trojan always runs over TLS: a share link of Trojan often leaves
    // `security` out. VLESS has no TLS unless the link asks.
    let default = if link.scheme == "trojan" { "tls" } else { "none" };
    let security = link.param("security").unwrap_or(default);
    let mut out = Outcome::default();
    let plan = match speaks(&link, security) {
        Ok(plan) => plan,
        Err(why) => {
            out.skip(why);
            return Ok(out);
        }
    };
    let (line, road) = tokio::time::timeout(net.limit * 2, async {
        if let Some(plan) = &plan {
            let request = vless_request(&link, &host, port)?;
            let (mut down, version) = xhttp::session(net, &link, plan, request).await?;
            return Ok((vless_banner(&mut down).await?, format!(" over XHTTP packet-up on {}", version.name())));
        }
        let io = open(net, &link, security).await?;
        let line = match link.scheme.as_str() {
            "trojan" => trojan(io, &link, &host, port).await,
            "vless" => vless(io, &link, &host, port).await,
            scheme => Err(Finding::failed(format!("the watch speaks no {scheme} link"))),
        }?;
        Ok::<_, Finding>((line, String::new()))
    })
    .await
    .map_err(|_| {
        Finding::failed(format!("no banner through {}:{} within {:?}", link.host, link.port, net.limit * 2))
    })??;
    out.saw(format!("{} {}:{}{road} to {host}:{port} gave {line:?}", link.scheme, link.host, link.port));
    Ok(out)
}

/// What the watch speaks of `link`: Trojan or VLESS over TCP (`None`), or
/// VLESS over XHTTP (its plan); else why not. A VLESS link with a flow or
/// an encryption asks for bytes that the watch does not send.
fn speaks(link: &Link, security: &str) -> Result<Option<Plan>, String> {
    let (scheme, transport) = (link.scheme.as_str(), link.param("type").unwrap_or("tcp"));
    let not = || format!("the watch speaks no {scheme} transport {transport} with security {security}");
    if !matches!(security, "tls" | "none") {
        return Err(not());
    }
    if scheme == "vless" {
        if let Some(flow) = link.param("flow").filter(|f| !f.is_empty()) {
            return Err(format!("the watch speaks no VLESS flow {flow}"));
        }
        if let Some(encryption) = link.param("encryption").filter(|e| !matches!(*e, "" | "none")) {
            return Err(format!("the watch speaks no VLESS encryption {encryption}"));
        }
    }
    match transport {
        // `raw` is the name of `tcp` in Xray's newer configurations.
        "tcp" | "raw" => Ok(None),
        // `splithttp` is the name of `xhttp` in Xray's older configurations.
        "xhttp" | "splithttp" if scheme == "vless" => Plan::of(link, security).map(Some),
        _ => Err(not()),
    }
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

/// VLESS over TCP: the request, then the answer and the banner.
async fn vless(mut io: BoxedIo, link: &Link, host: &str, port: u16) -> Result<String, Finding> {
    let request = vless_request(link, host, port)?;
    io.write_all(&request).await.map_err(|e| Finding::failed(format!("the VLESS request: {e}")))?;
    vless_banner(&mut io).await
}

/// The request of VLESS: version 0, the UUID, no addon, a CONNECT of TCP to
/// `host:port`.
fn vless_request(link: &Link, host: &str, port: u16) -> Result<Vec<u8>, Finding> {
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
    Ok(request)
}

/// The answer of VLESS: version 0, the length of its addons and the addons;
/// then the banner of the target.
async fn vless_banner<R: AsyncRead + Unpin>(io: &mut R) -> Result<String, Finding> {
    let mut head = [0u8; 2];
    io.read_exact(&mut head).await.map_err(|e| Finding::failed(format!("the VLESS answer: {e}")))?;
    if head[0] != 0 {
        return Err(Finding::failed(format!("the VLESS answer has the version {}", head[0])));
    }
    let mut addons = vec![0u8; usize::from(head[1])];
    io.read_exact(&mut addons).await.map_err(|e| Finding::failed(format!("the VLESS answer: {e}")))?;
    read_line(io, Vec::new()).await
}

/// The banner line of the target from `io`, after the bytes of `bytes`.
async fn read_line<R: AsyncRead + Unpin>(io: &mut R, mut bytes: Vec<u8>) -> Result<String, Finding> {
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
