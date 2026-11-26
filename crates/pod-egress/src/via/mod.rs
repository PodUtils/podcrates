//! A second proxy over TLS: a forward proxy that the user runs, as Caddy's
//! forwardproxy, squid's `https_port` or h2o, is the road in place of the
//! relay. The client opens TLS to it through the caller's way out (the
//! host's own proxy first when one is set); then it asks it for the target
//! with CONNECT inside that TLS, and the tunnel carries the session as a
//! TCP connection does.
//!
//! HTTP/2's CONNECT (RFC 9113, section 8.5) when the proxy chooses `h2` by
//! ALPN, each tunnel a stream, so that several share one connection
//! ([`Client`]); HTTP/1.1's (RFC 9110, section 9.3.6) otherwise, one
//! connection for each tunnel. A proxy that takes HTTP/2 and resets its
//! CONNECT gets HTTP/1.1 on a connection of its own. The credentials are
//! Basic, from the text of a private file that the caller read: a URL with a
//! password is refused, as argv and the list of processes would show it.
//!
//! [`Client`] is a [`pod_core::Dial`]: a dial of a target is a tunnel to it.

mod http1;
mod http2;

use std::time::Duration;

use tokio::time::Instant;

use base64::Engine as _;
use bytes::Bytes;
use pod_core::{BoxFuture, BoxedIo, Connected, Failure, Target};
use pod_proxy::DialError;
use pod_proxy::hop::ProxyTls;
use pod_proxy::host::{authority, check_name, split_host_port};
use pod_tls::Trust;
use pod_ws::Way;
use tokio_rustls::client::TlsStream;
use zeroize::Zeroizing;

pub use crate::Duplex;
/// A tunnel on a stream of HTTP/2: `pod-ws`'s byte stream of one stream.
pub use pod_ws::h2_road::Stream;

/// The second proxy's port when its URL names none.
pub const DEFAULT_PORT: u16 = 443;

/// The version of HTTP that carried a CONNECT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http1,
    Http2,
}

impl Version {
    /// The version as a report names it.
    pub fn name(self) -> &'static str {
        match self {
            Version::Http1 => "HTTP/1.1",
            Version::Http2 => "HTTP/2",
        }
    }
}

/// The versions that the client offers by ALPN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Offer {
    /// `h2`, then `http/1.1`: the proxy chooses.
    #[default]
    Both,
    /// `http/1.1` alone.
    Http1,
    /// `h2` alone: a proxy that does not choose it is refused.
    Http2,
}

impl Offer {
    /// `1.1` or `2`; empty or `auto` offers both.
    pub fn parse(text: &str) -> Result<Offer, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Offer::Both),
            "1.1" | "1" | "http/1.1" => Ok(Offer::Http1),
            "2" | "h2" | "http/2" => Ok(Offer::Http2),
            other => Err(format!("{other:?} is no version: 1.1, 2 or auto")),
        }
    }

    fn alpn(self) -> Vec<Vec<u8>> {
        match self {
            Offer::Both => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            Offer::Http1 => vec![b"http/1.1".to_vec()],
            Offer::Http2 => vec![b"h2".to_vec()],
        }
    }
}

/// A second proxy: `https://[USER@]HOST[:PORT]`, and the password that a
/// file gave. Neither the user nor the password is ever shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Via {
    pub host: String,
    pub port: u16,
    user: Option<String>,
    password: Option<Zeroizing<String>>,
}

impl std::fmt::Debug for Via {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Via")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("credentials", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl Via {
    /// Parse `https://[USER@]HOST[:PORT][/]`; the port is 443 when none is
    /// named. A password, another scheme, a path or a query is refused.
    pub fn parse(url: &str) -> Result<Via, String> {
        let url = url.trim();
        let Some((scheme, rest)) = url.split_once("://") else {
            return Err("give https://[USER@]HOST[:PORT]: the second proxy is reached over TLS".into());
        };
        if !scheme.eq_ignore_ascii_case("https") {
            return Err(format!(
                "{scheme}:// is not taken: the second proxy is reached over TLS, as https://HOST[:PORT]; a proxy \
                 in cleartext is HTTPS_PROXY's"
            ));
        }
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        if !(tail.is_empty() || tail == "/") {
            return Err("a proxy is named by its host and port alone: no path, query or fragment".into());
        }
        let (user, hostport) = match authority.rsplit_once('@') {
            // Before decoding: a `:` escaped in the user is the user's.
            Some((userinfo, _)) if userinfo.contains(':') => {
                return Err("the URL holds a password, which argv and the list of processes would show: give \
                            USER:PASSWORD in a private file instead"
                    .into());
            }
            Some((userinfo, hostport)) => {
                let user = pod_proxy::proxy::percent_decode(userinfo)?;
                // RFC 7617: a user-id holds no colon.
                if user.is_empty() || user.contains(':') || user.chars().any(char::is_control) {
                    return Err("the URL's user is empty, or holds a colon or a control character".into());
                }
                (Some(user), hostport)
            }
            None => (None, authority),
        };
        let (host, port) = split_host_port(hostport)?;
        if host.is_empty() {
            return Err("the URL names no host".into());
        }
        check_name(&host).map_err(|e| e.to_string())?;
        let port = port.unwrap_or(DEFAULT_PORT);
        if port == 0 {
            return Err("port 0 is no port".into());
        }
        Ok(Via { host, port, user, password: None })
    }

    /// The credentials from the text of a private file: its one line is
    /// `USER:PASSWORD`, or the password alone when the URL names the user. A
    /// refusal never holds the line.
    pub fn with_credentials(mut self, text: &str) -> Result<Via, String> {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        });
        let line = lines.next().ok_or("it has no line: give USER:PASSWORD")?;
        if lines.next().is_some() {
            return Err("it has more than one line: give one, USER:PASSWORD".into());
        }
        let (user, password) = match &self.user {
            Some(user) => (user.clone(), line),
            None => {
                let (user, password) = line.split_once(':').ok_or(
                    "its line holds no colon: give USER:PASSWORD, or name the user in the URL, https://USER@HOST",
                )?;
                (user.to_string(), password)
            }
        };
        if user.is_empty() || user.chars().any(char::is_control) || password.chars().any(char::is_control) {
            return Err("its user is empty, or the line holds a control character".into());
        }
        self.user = Some(user);
        self.password = Some(Zeroizing::new(password.to_string()));
        Ok(self)
    }

    /// The URL with no user: `https://HOST:PORT`.
    pub fn shown(&self) -> String {
        format!("https://{}", authority(&self.host, self.port))
    }

    /// Whether the URL names a user.
    pub fn names_user(&self) -> bool {
        self.user.is_some()
    }

    /// Whether credentials go with each CONNECT.
    pub fn has_credentials(&self) -> bool {
        self.password.is_some()
    }

    /// `Basic BASE64(USER:PASSWORD)`, for `Proxy-Authorization`.
    pub(crate) fn authorization(&self) -> Option<Zeroizing<String>> {
        let (user, password) = (self.user.as_ref()?, self.password.as_ref()?);
        let pair = Zeroizing::new(format!("{user}:{}", password.as_str()));
        let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(pair.as_bytes()));
        Some(Zeroizing::new(format!("Basic {}", encoded.as_str())))
    }
}

/// How a [`Client`] reaches its proxy.
#[derive(Debug, Clone)]
pub struct Dial {
    /// The way out: the dialer of the TCP to the proxy (the host's own
    /// proxy first when one is set), the TLS context and its words, and the
    /// user agent of each CONNECT.
    pub way: Way,
    /// The trust of the proxy's certificate, checked with its name.
    pub trust: Trust,
    /// The bound of each step: the TCP, TLS, HTTP/2's handshake, a CONNECT.
    pub timeout: Duration,
    pub offer: Offer,
    /// Where the user gives the credentials file, as the words of a `407`
    /// name it (podssh: `--via-auth-file FILE`).
    pub auth_file: String,
}

/// What a CONNECT says of the caller: its user agent, and its name and the
/// place of its credentials file in the words of a `407`.
pub(crate) struct Said<'a> {
    pub user_agent: &'a str,
    pub app: &'a str,
    pub auth_file: &'a str,
}

/// A tunnel to a target.
pub struct Opened {
    pub stream: Box<dyn Duplex>,
    pub version: Version,
    /// The tunnel is a stream of an HTTP/2 connection that an earlier one
    /// opened.
    pub shared: bool,
    /// Why HTTP/2 did not carry the CONNECT, when HTTP/1.1 then did.
    pub fell_back: Option<String>,
}

/// The connections to one second proxy: HTTP/2's is kept while it lasts,
/// and each tunnel that comes after it is a stream of it.
pub struct Client {
    via: Via,
    dial: Dial,
    shared: tokio::sync::Mutex<Option<::h2::client::SendRequest<Bytes>>>,
}

/// A connection for one CONNECT.
enum Connection {
    // Boxed: a TLS stream is far larger than a handle of HTTP/2.
    Http1(Box<TlsStream<BoxedIo>>),
    Http2 { sender: ::h2::client::SendRequest<Bytes>, shared: bool },
}

impl Client {
    pub fn new(via: Via, dial: Dial) -> Client {
        Client { via, dial, shared: tokio::sync::Mutex::new(None) }
    }

    pub fn via(&self) -> &Via {
        &self.via
    }

    fn said(&self) -> Said<'_> {
        let way = &self.dial.way;
        Said { user_agent: &way.user_agent, app: &way.tls.words().app, auth_file: &self.dial.auth_file }
    }

    /// A tunnel to `host:port`.
    pub async fn open(&self, host: &str, port: u16) -> Result<Opened, DialError> {
        check_name(host)?;
        let target = authority(host, port);
        let said = self.said();
        let (sender, shared) = match self.connection().await? {
            Connection::Http1(tls) => {
                let tls = http1::connect(tls, &self.via, &said, &target, self.dial.timeout).await?;
                return Ok(Opened { stream: Box::new(tls), version: Version::Http1, shared: false, fell_back: None });
            }
            Connection::Http2 { sender, shared } => (sender, shared),
        };
        let why = match http2::connect(sender, &self.via, &said, &target, self.dial.timeout).await {
            Ok(stream) => {
                return Ok(Opened { stream: Box::new(stream), version: Version::Http2, shared, fell_back: None });
            }
            Err(http2::Failed::Answer(e)) => return Err(e),
            Err(http2::Failed::Carried(why)) => why,
        };
        // The connection is not kept: the next tunnel makes a new one.
        *self.shared.lock().await = None;
        if self.dial.offer != Offer::Both {
            return Err(DialError::ProxyProtocol { proxy: self.via.shown(), detail: why });
        }
        let tls = self.tls(Offer::Http1).await?;
        let tls = http1::connect(tls, &self.via, &said, &target, self.dial.timeout).await?;
        Ok(Opened { stream: Box::new(tls), version: Version::Http1, shared: false, fell_back: Some(why) })
    }

    /// HTTP/2's connection that is kept, else a new connection; one is
    /// made at a time, so that tunnels that start together share it.
    async fn connection(&self) -> Result<Connection, DialError> {
        let mut slot = self.shared.lock().await;
        if let Some(sender) = slot.clone() {
            // A connection that ended is never ready again.
            let ready = tokio::time::timeout(self.dial.timeout, sender.ready()).await;
            if let Ok(Ok(sender)) = ready {
                return Ok(Connection::Http2 { sender, shared: true });
            }
            *slot = None;
        }
        let tls = self.tls(self.dial.offer).await?;
        if tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice()) {
            let sender = http2::handshake(tls, &self.via, self.dial.timeout).await?;
            *slot = Some(sender.clone());
            return Ok(Connection::Http2 { sender, shared: false });
        }
        if self.dial.offer == Offer::Http2 {
            let detail = "it did not choose HTTP/2 by ALPN (h2), which was asked for".to_string();
            return Err(DialError::ProxyProtocol { proxy: self.via.shown(), detail });
        }
        Ok(Connection::Http1(Box::new(tls)))
    }

    /// TCP to the proxy, through the host's own proxy when one is set, then
    /// TLS checked for the proxy's name, offering `offer`: the hop of
    /// `pod_proxy::hop::tls`, as an `https://` proxy of the host takes it.
    async fn tls(&self, offer: Offer) -> Result<TlsStream<BoxedIo>, DialError> {
        let (limit, way) = (self.dial.timeout, &self.dial.way);
        let tcp = way.tcp(&self.via.host, self.via.port, limit).await?.io;
        let tls = ProxyTls { context: way.tls.clone(), trust: self.dial.trust.clone() };
        let alpn = offer.alpn();
        let alpn: Vec<&[u8]> = alpn.iter().map(Vec::as_slice).collect();
        pod_proxy::hop::tls(tcp, &self.via.shown(), &self.via.host, &tls, &alpn, limit).await
    }
}

/// The proxy's refusal of a CONNECT to `target`; a `407` says what it asks
/// for, and whether credentials went with it. Another refusal of a CONNECT
/// that carried credentials may be a probe-resistant proxy's (NaiveProxy's
/// forwardproxy), which answers as its web server does where its
/// credentials do not match, so as to look like no proxy at all.
fn refused(via: &Via, said: &Said<'_>, target: &str, status: u16, reason: String, head: &str) -> DialError {
    let reason = match status {
        407 => explain(&reason, head, via.has_credentials(), said),
        200..=299 => reason,
        _ if via.has_credentials() => format!(
            "{reason}; a proxy with probe resistance answers so where its credentials do not match: check the \
             credentials file"
        ),
        _ => reason,
    };
    DialError::ProxyRefused { proxy: via.shown(), target: target.to_string(), status, reason }
}

/// The words of a `407`: the schemes of its `Proxy-Authenticate`, and what
/// the caller gave. `head` is a response's head, its first line the status.
pub(crate) fn explain(reason: &str, head: &str, sent: bool, said: &Said<'_>) -> String {
    let reason = if reason.trim().is_empty() { "Proxy Authentication Required" } else { reason };
    let asks = pod_proxy::proxy_auth::schemes(head);
    let list = match asks.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let basic = asks.is_empty() || asks.iter().any(|s| s.eq_ignore_ascii_case("basic"));
    let what = match (basic, sent) {
        (true, true) => "it refused the user and password of the credentials file (Basic)".to_string(),
        (true, false) => {
            format!("{} sent none: give USER:PASSWORD in a file of mode 0600 ({})", said.app, said.auth_file)
        }
        (false, _) => format!("{} sends Basic only, from the credentials file", said.app),
    };
    match asks.is_empty() {
        true => format!("{reason}; {what}"),
        false => format!("{reason}; it asks for {list}, and {what}"),
    }
}

impl pod_core::Dial for Client {
    fn label(&self) -> String {
        format!("via {}", self.via.shown())
    }

    fn proxy_label(&self, _target: &Target) -> Option<String> {
        Some(self.via.shown())
    }

    /// A tunnel to `target`, the whole of it before `deadline`.
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            crate::road::left(deadline, target)?;
            let opened = tokio::time::timeout_at(deadline, self.open(&target.host(), target.port()))
                .await
                .map_err(|_| crate::road::late(target, &self.via.shown()))??;
            Ok(Connected::new(Box::new(opened.stream)))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
