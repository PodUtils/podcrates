//! The tunnel through an HTTP proxy: `CONNECT name:port`, with the user and
//! the password of the proxy URL as Basic, and the head of the answer read
//! to its end and no further.

use base64::Engine as _;
use pod_core::Stage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::DialError;
use crate::host::{authority, check_name};
use crate::proxy::Proxy;
use crate::race::{Leg, connect_direct};

/// The longest CONNECT response head that is read before giving up.
pub const MAX_PROXY_RESPONSE: usize = 16 * 1024;

/// The words of the consumer in a CONNECT and in its refusal.
#[derive(Clone, Copy)]
pub(crate) struct Ask<'a> {
    /// The program, in the words of a 407.
    pub app: &'a str,
    /// The `User-Agent` of the request; none is sent with `None`.
    pub user_agent: Option<&'a str>,
}

/// Connect to the proxy and ask it for a tunnel to `host:port`.
pub(crate) async fn connect_via(
    leg: Leg<'_>,
    ask: Ask<'_>,
    proxy: &Proxy,
    host: &str,
    port: u16,
) -> Result<TcpStream, DialError> {
    check_name(&proxy.host)?;
    let proxy_name = proxy.to_string();
    let target = authority(host, port);
    let budget = leg.budget;
    let mut stream = connect_direct(leg, &proxy.host, proxy.port).await.map_err(|e| match e {
        DialError::Timeout { .. } => DialError::Timeout {
            step: format!("connecting to the proxy {proxy_name}"),
            after: budget,
            stage: Stage::Proxy,
        },
        other => DialError::ProxyUnreachable { proxy: proxy_name.clone(), detail: other.to_string() },
    })?;

    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(agent) = ask.user_agent {
        request.push_str(&format!("User-Agent: {agent}\r\n"));
    }
    if let Some((user, password)) = proxy.credentials() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}").as_bytes());
        request.push_str(&format!("Proxy-Authorization: Basic {encoded}\r\n"));
    }
    request.push_str("\r\n");

    let exchange = async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        read_head_exact(&mut stream).await
    };
    let head = tokio::time::timeout_at(leg.deadline, exchange)
        .await
        .map_err(|_| DialError::Timeout {
            step: format!("the CONNECT exchange with {proxy_name}"),
            after: budget,
            stage: Stage::Proxy,
        })?
        .map_err(|e| DialError::ProxyProtocol { proxy: proxy_name.clone(), detail: e.to_string() })?;

    let (status, reason) = crate::http::parse_status_line(&head)
        .map_err(|detail| DialError::ProxyProtocol { proxy: proxy_name.clone(), detail })?;
    if (200..300).contains(&status) {
        Ok(stream)
    } else {
        // A 407 says why with the schemes that the proxy takes.
        let reason = if status == 407 {
            crate::proxy_auth::explain(&reason, &head, proxy.credentials().is_some(), ask.app)
        } else {
            reason
        };
        Err(DialError::ProxyRefused { proxy: proxy_name, target, status, reason })
    }
}

/// Read a response head one byte at a time, so nothing after `\r\n\r\n` is
/// consumed: those bytes belong to the TLS session that follows.
async fn read_head_exact(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut head = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte).await? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the connection closed before the response ended",
            ));
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&head).into_owned());
        }
        if head.len() > MAX_PROXY_RESPONSE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("no end of headers within {MAX_PROXY_RESPONSE} bytes"),
            ));
        }
    }
}
