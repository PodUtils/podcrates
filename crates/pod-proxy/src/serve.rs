//! The server face of a proxy that a consumer serves: the SOCKS5 greeting,
//! the user and the password of RFC 1929, the request and the reply of
//! RFC 1928; the request of an HTTP proxy (`CONNECT HOST:PORT`, or an
//! `http://` URL to forward in origin form). The caller dials the target
//! and copies the bytes. The client face is the dial ([`crate::Dialer`]);
//! the datagrams of a UDP association are [`crate::udp`] ([`crate::udp::unwrap`]
//! reads a datagram of a client).

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pod_core::{Class, Failure, Target};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::credentials::Credentials;
use crate::http::request::{Request, RequestError, connect_target, read_request};

/// The version byte of SOCKS5: the first byte of its greeting.
pub const SOCKS5: u8 = 5;
/// The command CONNECT.
pub const CMD_CONNECT: u8 = 0x01;
pub use crate::udp::{CMD_UDP_ASSOCIATE, CMD_UDP_IN_STREAM, REPLY_COMMAND_NOT_SUPPORTED};
/// The replies of RFC 1928, section 6.
pub const REPLY_SUCCEEDED: u8 = 0x00;
pub const REPLY_GENERAL_FAILURE: u8 = 0x01;
pub const REPLY_NOT_ALLOWED: u8 = 0x02;
pub const REPLY_HOST_UNREACHABLE: u8 = 0x04;
pub const REPLY_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;

const NO_AUTHENTICATION: u8 = 0x00;
const USER_PASSWORD: u8 = 0x02;
const NO_ACCEPTABLE_METHOD: u8 = 0xff;
const IPV4: u8 = 0x01;
const NAME: u8 = 0x03;
const IPV6: u8 = 0x04;

/// True when `first`, the first byte of a client, starts a SOCKS5
/// greeting, not an HTTP request.
pub fn looks_like_socks5(first: u8) -> bool {
    first == SOCKS5
}

fn bad(text: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text.into())
}

/// Read the greeting of a SOCKS5 client and one request: the command (1
/// CONNECT, 3 UDP ASSOCIATE, 0x83 UDP in the stream) and the destination.
/// With `credentials`, the method of RFC 1929 is required and the user and
/// the password must match; else the method "no authentication" is. A
/// client that offers no such method gets `0xff`; a wrong user or password
/// gets the status 1. The caller answers the request with
/// [`socks5_reply`].
///
/// # Errors
///
/// `InvalidData` for a break of the protocol, `PermissionDenied` for a
/// refused method or a wrong user or password, or the error of the stream.
pub async fn read_socks5<S>(stream: &mut S, credentials: Option<&Credentials>) -> io::Result<(u8, Target)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut intro = [0u8; 2];
    stream.read_exact(&mut intro).await?;
    if intro[0] != SOCKS5 {
        return Err(bad(format!("SOCKS5 version {} is not 5", intro[0])));
    }
    let mut methods = vec![0u8; usize::from(intro[1])];
    stream.read_exact(&mut methods).await?;
    let wanted = if credentials.is_some() { USER_PASSWORD } else { NO_AUTHENTICATION };
    if !methods.contains(&wanted) {
        stream.write_all(&[SOCKS5, NO_ACCEPTABLE_METHOD]).await?;
        let needs = if wanted == USER_PASSWORD { "user and password" } else { "no authentication" };
        let text = format!("the SOCKS5 client offers no acceptable method (it needs {needs})");
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, text));
    }
    stream.write_all(&[SOCKS5, wanted]).await?;
    if let Some(credentials) = credentials {
        check_user(stream, credentials).await?;
    }
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[0] != SOCKS5 {
        return Err(bad(format!("SOCKS5 request version {} is not 5", head[0])));
    }
    let host = match head[3] {
        IPV4 => {
            let mut a = [0u8; 4];
            stream.read_exact(&mut a).await?;
            IpAddr::from(a).to_string()
        }
        IPV6 => {
            let mut a = [0u8; 16];
            stream.read_exact(&mut a).await?;
            IpAddr::from(a).to_string()
        }
        NAME => {
            let len = stream.read_u8().await?;
            let mut name = vec![0u8; usize::from(len)];
            stream.read_exact(&mut name).await?;
            let name = String::from_utf8(name).map_err(|_| bad("the SOCKS5 name is not UTF-8"))?;
            if name.is_empty() {
                return Err(bad("the SOCKS5 name is empty"));
            }
            name
        }
        other => {
            socks5_reply(stream, REPLY_ADDRESS_TYPE_NOT_SUPPORTED, None).await?;
            return Err(bad(format!("SOCKS5 address type {other} is not supported")));
        }
    };
    let port = stream.read_u16().await?;
    Ok((head[1], Target::new(&host, port)))
}

/// RFC 1929: version 1, the user, the password; the status 0 or 1.
async fn check_user<S>(stream: &mut S, credentials: &Credentials) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x01 {
        return Err(bad(format!("SOCKS5 user and password version {} is not 1", head[0])));
    }
    let mut user = vec![0u8; usize::from(head[1])];
    stream.read_exact(&mut user).await?;
    let mut pass = vec![0u8; usize::from(stream.read_u8().await?)];
    stream.read_exact(&mut pass).await?;
    let ok = credentials.check(&String::from_utf8_lossy(&user), &String::from_utf8_lossy(&pass));
    stream.write_all(&[0x01, if ok { 0x00 } else { 0x01 }]).await?;
    if !ok {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the SOCKS5 user or password is wrong"));
    }
    Ok(())
}

/// Send the reply `code` with the bound address `bound` (0.0.0.0:0 when
/// there is none).
///
/// # Errors
///
/// The error of the write.
pub async fn socks5_reply<S>(stream: &mut S, code: u8, bound: Option<SocketAddr>) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let bound = bound.unwrap_or(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
    let mut out = vec![SOCKS5, code, 0x00];
    match bound.ip() {
        IpAddr::V4(v4) => {
            out.push(IPV4);
            out.extend_from_slice(&v4.octets());
        }
        IpAddr::V6(v6) => {
            out.push(IPV6);
            out.extend_from_slice(&v6.octets());
        }
    }
    out.extend_from_slice(&bound.port().to_be_bytes());
    stream.write_all(&out).await?;
    stream.flush().await
}

/// The reply of a dial that failed: `0x02` for a policy (a rule refused
/// the destination), `0x04` for a destination that did not answer, has no
/// address or refused the connection, `0x01` for the rest.
pub fn reply_for(failure: &Failure) -> u8 {
    match failure.class {
        Class::Policy => REPLY_NOT_ALLOWED,
        Class::Timeout | Class::NotFound | Class::Unavailable | Class::Refused => REPLY_HOST_UNREACHABLE,
        _ => REPLY_GENERAL_FAILURE,
    }
}

/// The request of a client of an HTTP proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpProxyRequest {
    /// `CONNECT HOST:PORT`: answer it, then copy the bytes.
    Connect(Target),
    /// A request for an `http://` URL: write `head` (the request in origin
    /// form, with no `Proxy-*`, `Connection` and `Keep-Alive` header and
    /// with `Connection: close`) to the origin, then copy the bytes.
    Forward { target: Target, head: Vec<u8> },
}

/// One request of a client of an HTTP proxy, read with
/// [`read_http_proxy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpOpening {
    pub request: HttpProxyRequest,
    /// The value of `Proxy-Authorization`, for
    /// [`Credentials::check_basic`].
    pub authorization: Option<String>,
    /// The bytes after the head: the start of a body, or of the bytes of a
    /// tunnel. They go to the target after the head.
    pub rest: Vec<u8>,
}

/// Read the request of a client of an HTTP proxy: `CONNECT`, or a request
/// for an `http://` URL (absolute form, port 80 by default). The caller
/// answers an error that has a status with
/// [`crate::http::request::refusal`].
///
/// # Errors
///
/// The [`RequestError`] of the head; a target that is not `HOST:PORT`, or
/// not an `http://` URL, is `Malformed` with the status 400.
pub async fn read_http_proxy<S>(stream: &mut S) -> Result<HttpOpening, RequestError>
where
    S: AsyncRead + Unpin,
{
    let (request, rest) = read_request(stream, Vec::new()).await?;
    let authorization = request.header("proxy-authorization").map(str::to_string);
    let malformed = |why: String| RequestError::Malformed { status: 400, why };
    if request.method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = connect_target(&request.target).map_err(malformed)?;
        let request = HttpProxyRequest::Connect(Target::new(&host, port));
        return Ok(HttpOpening { request, authorization, rest });
    }
    let (target, head) = forward_head(&request).map_err(malformed)?;
    Ok(HttpOpening { request: HttpProxyRequest::Forward { target, head }, authorization, rest })
}

/// The target of a request for an `http://` URL, and the request in origin
/// form for the origin.
fn forward_head(request: &Request) -> Result<(Target, Vec<u8>), String> {
    let url = &request.target;
    let Some(rest) = url.get(..7).filter(|s| s.eq_ignore_ascii_case("http://")).map(|_| &url[7..]) else {
        return Err(format!("{url:?}: this proxy forwards http:// URLs; use CONNECT for https://"));
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('/') => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let with_port = if authority.rsplit_once(':').is_some_and(|(_, p)| !p.contains(']')) {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let (host, port) = connect_target(&with_port).map_err(|e| format!("the URL {url:?}: {e}"))?;
    let mut head = format!("{} {path} HTTP/1.{}\r\n", request.method, request.minor);
    for (name, value) in &request.headers {
        if name.starts_with("proxy-") || name == "connection" || name == "keep-alive" {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    Ok((Target::new(&host, port), head.into_bytes()))
}
