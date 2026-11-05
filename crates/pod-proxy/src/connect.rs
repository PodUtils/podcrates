//! The tunnel through an HTTP proxy: `CONNECT name:port`, with the user and
//! the password of the proxy URL as Basic, and the head of the answer read
//! to its end and no further. A `407` with a Digest challenge gets the
//! Digest answer on a new connection. A refusal keeps the proxy's reason
//! phrase and the first line of its body.

use base64::Engine as _;
use pod_core::Stage;
use pod_core::entropy::OsEntropy;
use pod_core::text::one_line;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::error::DialError;
use crate::host::authority;
use crate::proxy::Proxy;
use crate::proxy_auth::Sent;
use crate::race::Leg;
use crate::stream::{Stream, open_proxy};

/// The longest CONNECT response head that is read before giving up.
pub const MAX_PROXY_RESPONSE: usize = 16 * 1024;

/// The most bytes of the body of a refusal that are read for its first line.
const MAX_REASON_BODY: usize = 512;

/// The words of the consumer in a CONNECT and in its refusal.
#[derive(Clone, Copy)]
pub(crate) struct Ask<'a> {
    /// The program, in the words of a 407.
    pub app: &'a str,
    /// The `User-Agent` of the request; none is sent with `None`.
    pub user_agent: Option<&'a str>,
}

/// How one CONNECT ended, short of an error.
enum Answer {
    /// 2xx: the tunnel is open.
    Open,
    /// A 407 with a Digest challenge that the credentials can answer.
    Digest(crate::digest::Challenge),
}

/// Connect to the proxy and ask it for a tunnel to `host:port`.
pub(crate) async fn connect_via(
    leg: Leg<'_>,
    ask: Ask<'_>,
    proxy: &Proxy,
    host: &str,
    port: u16,
) -> Result<Stream, DialError> {
    let target = authority(host, port);
    let basic = proxy.credentials().map(|(user, password)| {
        let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}").as_bytes());
        format!("Basic {encoded}")
    });
    let mut stream = open_proxy(leg, proxy).await?;
    let challenge = match exchange(&mut stream, leg, ask, proxy, &target, basic.as_deref(), Sent::Basic).await? {
        Answer::Open => return Ok(stream),
        Answer::Digest(challenge) => challenge,
    };
    // A Digest proxy: the answer to its challenge goes on a new connection.
    // A challenge comes only after Basic, which only credentials send.
    let (user, password) = proxy.credentials().unwrap_or_default();
    let nonce = crate::digest::cnonce(&mut OsEntropy).map_err(|e| DialError::ProxyProtocol {
        proxy: proxy.to_string(),
        detail: format!("the Digest answer needs a client nonce: {e}"),
    })?;
    let answer = crate::digest::authorization(&challenge, user, password, "CONNECT", &target, &nonce, 1);
    let mut stream = open_proxy(leg, proxy).await?;
    match exchange(&mut stream, leg, ask, proxy, &target, Some(&answer), Sent::Digest).await? {
        Answer::Open => Ok(stream),
        // A Digest answer gets no challenge back; a 407 is a refusal.
        Answer::Digest(_) => Err(DialError::ProxyRefused {
            proxy: proxy.to_string(),
            target,
            status: 407,
            reason: "the proxy refused the Digest answer".into(),
        }),
    }
}

/// One CONNECT with `authorization`, the value of `Proxy-Authorization`,
/// which is `sent`. A 407 with a Digest challenge, after Basic with the
/// credentials of the URL, gives the challenge.
#[allow(clippy::too_many_arguments)]
async fn exchange(
    stream: &mut Stream,
    leg: Leg<'_>,
    ask: Ask<'_>,
    proxy: &Proxy,
    target: &str,
    authorization: Option<&str>,
    sent: Sent,
) -> Result<Answer, DialError> {
    let proxy_name = proxy.to_string();
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: keep-alive\r\n");
    if let Some(agent) = ask.user_agent {
        request.push_str(&format!("User-Agent: {agent}\r\n"));
    }
    if let Some(value) = authorization {
        request.push_str(&format!("Proxy-Authorization: {value}\r\n"));
    }
    request.push_str("\r\n");

    let exchange = async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        read_head_exact(stream).await
    };
    let head = tokio::time::timeout_at(leg.deadline, exchange)
        .await
        .map_err(|_| DialError::Timeout {
            step: format!("the CONNECT exchange with {proxy_name}"),
            after: leg.budget,
            stage: Stage::Proxy,
        })?
        .map_err(|e| DialError::ProxyProtocol { proxy: proxy_name.clone(), detail: e.to_string() })?;

    let (status, reason) = crate::http::parse_status_line(&head)
        .map_err(|detail| DialError::ProxyProtocol { proxy: proxy_name.clone(), detail })?;
    if (200..300).contains(&status) {
        return Ok(Answer::Open);
    }
    let reason = if status == 407 {
        let sent = if authorization.is_none() { Sent::Nothing } else { sent };
        if sent == Sent::Basic
            && let Some(challenge) = crate::digest::challenge_in(&head)
        {
            return Ok(Answer::Digest(challenge));
        }
        // A 407 says why with the schemes that the proxy takes.
        crate::proxy_auth::explain(&reason, &head, sent, ask.app)
    } else {
        // The proxy's own words, as "not on the egress allowlist", are
        // often in its body.
        let body = tokio::time::timeout_at(leg.deadline, first_line_of_body(stream, &head)).await.unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for part in [reason, body] {
            if !part.is_empty() && !parts.contains(&part) {
                parts.push(part);
            }
        }
        parts.join(": ")
    };
    Err(DialError::ProxyRefused { proxy: proxy_name, target: target.to_string(), status, reason })
}

/// The first line of the body of a refusal, made safe to print: at most
/// [`MAX_REASON_BODY`] bytes, by its `Content-Length` when it has one, each
/// read within 2 s.
async fn first_line_of_body<S: AsyncRead + Unpin>(stream: &mut S, head: &str) -> String {
    let length = head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
    });
    let want = length.unwrap_or(MAX_REASON_BODY).min(MAX_REASON_BODY);
    let mut body = Vec::new();
    let mut chunk = [0u8; MAX_REASON_BODY];
    while body.len() < want {
        match tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => body.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    body.truncate(want);
    let text = String::from_utf8_lossy(&body);
    text.lines().map(one_line).find(|l| !l.is_empty()).unwrap_or_default()
}

/// Read a response head one byte at a time, so nothing after `\r\n\r\n` is
/// consumed: those bytes belong to the TLS session that follows.
async fn read_head_exact<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<String> {
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
