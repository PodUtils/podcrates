//! The server's side of the opening handshake, for a listener of
//! WebSockets: the checks of RFC 6455 section 4.2.1 on a request that
//! `pod_proxy::http::request` read, the 101 of section 4.2.2, and the
//! session that writes frames as a server does. No extension is chosen. A
//! subprotocol is chosen only where the caller names one
//! ([`check_protocol`], [`switching_to`]); else a client that offered one
//! gets none, as the RFC allows. A request with no key opens raw bytes
//! where the caller asks for that, for a client of WebTunnel's shape; a
//! relay role never does.

use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncWrite};

use pod_proxy::http::request::{self, Request};

use crate::client::WRITE_TIMEOUT;
use crate::handshake;
use crate::session::RelaySession;

/// An upgrade that is refused: its status, its words and its headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub status: u16,
    pub why: String,
    pub headers: Vec<(&'static str, &'static str)>,
}

impl Refused {
    /// The answer, after which the connection closes.
    pub fn answer(&self) -> Vec<u8> {
        request::refusal(self.status, &self.why, &self.headers)
    }
}

fn refused<T>(status: u16, why: &str, headers: Vec<(&'static str, &'static str)>) -> Result<T, Refused> {
    Err(Refused { status, why: why.into(), headers })
}

/// What a good upgrade opens: frames by RFC 6455, with the key that the
/// accept answers, or raw bytes for a client that sent no key, as a client
/// of WebTunnel's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgrade {
    Frames(String),
    Raw,
}

/// The key of a client's upgrade, once it is one that RFC 6455 section 4.2.1
/// takes: GET, HTTP/1.1, a Host, `Upgrade: websocket`, `Connection:
/// Upgrade`, version 13 and a key of 16 bytes. A request with an `Origin` is
/// a browser's, and is refused (403): a page that the user opens could else
/// reach the other side of the pipe, which no program of the user's needs.
pub fn check_upgrade(head: &Request) -> Result<String, Refused> {
    opening(head)?;
    let key = keyed(head)?;
    no_origin(head)?;
    Ok(key)
}

/// [`check_upgrade`], or the raw form of a request with no key, for
/// a listener whose user asked for it: the method, the Host and the two
/// tokens as there; then the key decides, before the version, as a client
/// of WebTunnel's shape sends neither. A key takes the version and the
/// key's checks (a key that is there but not 16 bytes stays 400: a broken
/// client is not carried raw); no key is the raw form. An `Origin` is
/// refused in both, as a page could else reach the other side.
pub fn check_upgrade_or_raw(head: &Request) -> Result<Upgrade, Refused> {
    opening(head)?;
    let upgrade = if head.has("sec-websocket-key") { Upgrade::Frames(keyed(head)?) } else { Upgrade::Raw };
    no_origin(head)?;
    Ok(upgrade)
}

/// GET, HTTP/1.1 that names its Host, and the two tokens of an upgrade.
fn opening(head: &Request) -> Result<(), Refused> {
    if head.method != "GET" {
        return refused(405, "a WebSocket opens with GET", vec![("Allow", "GET")]);
    }
    if head.minor < 1 || !head.has("host") {
        return refused(400, "a WebSocket opens with a request of HTTP/1.1 that names its Host", vec![]);
    }
    if !asks_upgrade(head) {
        let headers = vec![("Upgrade", "websocket"), ("Connection", "Upgrade")];
        return refused(426, "this is a WebSocket: Upgrade: websocket and Connection: Upgrade", headers);
    }
    Ok(())
}

/// Version 13 and a key of 16 bytes: the key.
fn keyed(head: &Request) -> Result<String, Refused> {
    if head.header("sec-websocket-version") != Some("13") {
        return refused(426, "this server speaks version 13 of RFC 6455", vec![("Sec-WebSocket-Version", "13")]);
    }
    let key = head.header("sec-websocket-key").unwrap_or_default();
    let bytes = base64::engine::general_purpose::STANDARD.decode(key).map(|k| k.len()).unwrap_or(0);
    if bytes != 16 {
        return refused(400, "Sec-WebSocket-Key is not 16 bytes in base64", vec![]);
    }
    Ok(key.to_string())
}

/// No `Origin`: a request with one is a browser's.
fn no_origin(head: &Request) -> Result<(), Refused> {
    if head.has("origin") {
        return refused(403, "a request with an Origin is a browser's, which this pipe does not serve", vec![]);
    }
    Ok(())
}

/// Whether a request asks for a WebSocket at all: `Upgrade: websocket` and
/// `Connection: Upgrade`. One that does not is a browser's visit or another
/// protocol's, which a listener may answer with a page of its own.
pub fn asks_upgrade(head: &Request) -> bool {
    head.has_token("upgrade", "websocket") && head.has_token("connection", "upgrade")
}

/// The answer that opens the session (RFC 6455 section 4.2.2).
pub fn switching(key: &str) -> Vec<u8> {
    switching_to(key, None)
}

/// [`switching`], naming `protocol` in `Sec-WebSocket-Protocol` when one
/// is given: the subprotocol that [`check_protocol`] found in the request.
pub fn switching_to(key: &str, protocol: Option<&str>) -> Vec<u8> {
    let accept = handshake::accept_key(key);
    let mut head = String::from("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n");
    if let Some(protocol) = protocol {
        head.push_str(&format!("Sec-WebSocket-Protocol: {protocol}\r\n"));
    }
    format!("{head}Sec-WebSocket-Accept: {accept}\r\n\r\n").into_bytes()
}

/// Whether `name` can be a subprotocol: visible ASCII with no comma, so
/// that it cannot break a header line or a list of the header. RFC 6455
/// section 4.1 asks for a token, which has no `/`; podnet's `podnet/1`
/// has one, and a list of the header still reads it as one item.
pub fn is_protocol_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| matches!(b, 0x21..=0x7e) && b != b',')
}

/// Whether the request offers `protocol` in `Sec-WebSocket-Protocol`: in
/// any of its lists, compared exactly, as a subprotocol name is
/// case-sensitive.
pub fn offers(head: &Request, protocol: &str) -> bool {
    head.headers
        .iter()
        .filter(|(name, _)| name == "sec-websocket-protocol")
        .flat_map(|(_, value)| value.split(','))
        .any(|item| item.trim() == protocol)
}

/// A request that does not offer `protocol`, the only subprotocol of this
/// server, is refused (426), and the refusal names it.
pub fn check_protocol(head: &Request, protocol: &'static str) -> Result<(), Refused> {
    if !offers(head, protocol) {
        return refused(
            426,
            "this server speaks one subprotocol, which the request did not offer",
            vec![("Sec-WebSocket-Protocol", protocol)],
        );
    }
    Ok(())
}

/// The answer that opens raw bytes: 101 with the two headers and no
/// accept, as a WebTunnel server answers and its client checks.
pub fn switching_raw() -> Vec<u8> {
    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n".to_vec()
}

/// The session of a client whose upgrade was answered, `pending` the bytes
/// that came after its request: frames written unmasked, and only masked
/// ones taken (RFC 6455 section 5.1); an unmasked one closes with 1002.
pub fn session<S>(stream: S, pending: Vec<u8>, idle: Option<Duration>) -> RelaySession<S>
where
    S: AsyncRead + AsyncWrite,
{
    RelaySession::new(stream, pending, idle, WRITE_TIMEOUT).as_server().naming("the client")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(extra: &str) -> Request {
        let text = format!(
            "GET /chat HTTP/1.1\r\nHost: server.example.com\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
        );
        request::parse_request(text.as_bytes()).unwrap()
    }

    #[test]
    fn the_upgrade_of_rfc6455_section_1_3_opens_with_its_accept() {
        let key = check_upgrade(&head("")).unwrap();
        let answer = String::from_utf8(switching(&key)).unwrap();
        assert!(answer.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{answer}");
        assert!(answer.contains("\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"), "{answer}");
        assert!(!answer.to_ascii_lowercase().contains("sec-websocket-protocol"), "{answer}");
        let offered = check_upgrade(&head("Sec-WebSocket-Protocol: chat\r\nConnection: keep-alive\r\n"));
        assert!(offered.is_ok(), "a subprotocol offered is answered with none");
    }

    #[test]
    fn an_upgrade_that_rfc6455_refuses_gets_its_status() {
        let status = |text: &str| check_upgrade(&request::parse_request(text.as_bytes()).unwrap()).unwrap_err().status;
        let ok = "Host: h\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n";
        let key = "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";
        assert_eq!(status(&format!("POST / HTTP/1.1\r\n{ok}{key}\r\n")), 405);
        assert_eq!(status(&format!("GET / HTTP/1.0\r\n{ok}{key}\r\n")), 400);
        assert_eq!(status("GET / HTTP/1.1\r\nHost: h\r\n\r\n"), 426);
        assert_eq!(status(&format!("GET / HTTP/1.1\r\n{}{key}\r\n", ok.replace(": 13", ": 8"))), 426);
        assert_eq!(status(&format!("GET / HTTP/1.1\r\n{ok}Sec-WebSocket-Key: c2hvcnQ=\r\n\r\n")), 400);
        assert_eq!(status(&format!("GET / HTTP/1.1\r\n{ok}\r\n")), 400);
        assert_eq!(status(&format!("GET / HTTP/1.1\r\n{ok}{key}Origin: https://a.example\r\n\r\n")), 403);
        let refused = check_upgrade(&head("Origin: null\r\n")).unwrap_err();
        assert!(String::from_utf8(refused.answer()).unwrap().starts_with("HTTP/1.1 403 Forbidden: "));
    }

    /// A server of one subprotocol takes a request that offers it in any of
    /// its lists, exactly, and names it in the 101; it refuses another
    /// request with 426 and the name.
    #[test]
    fn a_server_of_one_subprotocol_takes_only_a_request_that_offers_it() {
        let p = "podnet/1";
        assert_eq!(check_protocol(&head("Sec-WebSocket-Protocol: podnet/1\r\n"), p), Ok(()));
        assert_eq!(check_protocol(&head("Sec-WebSocket-Protocol: chat, podnet/1\r\n"), p), Ok(()));
        let two = "Sec-WebSocket-Protocol: chat\r\nSec-WebSocket-Protocol: podnet/1\r\n";
        assert_eq!(check_protocol(&head(two), p), Ok(()), "a list in two headers");
        for other in ["", "Sec-WebSocket-Protocol: chat\r\n", "Sec-WebSocket-Protocol: PODNET/1\r\n"] {
            let refused = check_protocol(&head(other), p).unwrap_err();
            assert_eq!(refused.status, 426, "{other:?}");
            let answer = String::from_utf8(refused.answer()).unwrap();
            assert!(answer.contains("\r\nSec-WebSocket-Protocol: podnet/1\r\n"), "{answer}");
        }
        let answer = String::from_utf8(switching_to("dGhlIHNhbXBsZSBub25jZQ==", Some(p))).unwrap();
        assert!(answer.contains("\r\nSec-WebSocket-Protocol: podnet/1\r\n"), "{answer}");
        assert!(answer.ends_with("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n"), "{answer}");
        assert!(is_protocol_name(p) && is_protocol_name("chat"));
        for bad in ["", "a b", "a,b", "a\r\nX: 1", "a\tb", "é"] {
            assert!(!is_protocol_name(bad), "{bad:?}");
        }
    }

    /// A visit asks for no upgrade, and neither does a request with one of the
    /// two tokens alone; any case and a list hold them.
    #[test]
    fn a_visit_asks_for_no_upgrade() {
        let asks = |text: &str| asks_upgrade(&request::parse_request(text.as_bytes()).unwrap());
        assert!(asks("GET / HTTP/1.1\r\nHost: h\r\nUpgrade: WebSocket\r\nConnection: keep-alive, upgrade\r\n\r\n"));
        assert!(!asks("GET / HTTP/1.1\r\nHost: h\r\nAccept: text/html\r\n\r\n"));
        assert!(!asks("GET / HTTP/1.1\r\nHost: h\r\nUpgrade: websocket\r\n\r\n"));
        assert!(!asks("GET / HTTP/1.1\r\nHost: h\r\nConnection: Upgrade\r\nUpgrade: h2c\r\n\r\n"));
    }

    /// The request of a WebTunnel client, as podssh's `webtunnel::request`
    /// writes it for `url=http://bridge.example/secret-path`: the bytes of
    /// the road, until the road comes here with `pod-egress`.
    const WEBTUNNEL_REQUEST: &str = "GET /secret-path HTTP/1.1\r\nHost: bridge.example\r\n\
         User-Agent: Go-http-client/1.1\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n";

    /// The check of the answer of a WebTunnel client, as podssh's
    /// `webtunnel::check_answer` makes it: `101 Switching Protocols`,
    /// `Upgrade: websocket` and `Connection: upgrade`, in any case.
    fn webtunnel_takes(head: &str) -> Result<(), String> {
        let (status, reason, headers) = pod_proxy::http::parse_head(head)?;
        if status != 101 || reason != "Switching Protocols" {
            return Err(format!("{status} {reason}"));
        }
        let value = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.to_ascii_lowercase());
        match (value("upgrade").as_deref(), value("connection").as_deref()) {
            (Some("websocket"), Some("upgrade")) => Ok(()),
            _ => Err("no Upgrade: websocket and Connection: upgrade".into()),
        }
    }

    /// The request of a WebTunnel client, with no key and no version: the
    /// raw form where the caller asks for it, 426 where it does not; a key
    /// keeps each check of a framed upgrade, and an `Origin` is 403 either
    /// way.
    #[test]
    fn a_request_with_no_key_opens_raw_bytes_where_its_caller_asks() {
        let webtunnel = WEBTUNNEL_REQUEST.to_string();
        let parse = |text: &str| request::parse_request(text.as_bytes()).unwrap();
        let keyless = parse(&webtunnel);
        assert!(!keyless.has("sec-websocket-key") && !keyless.has("sec-websocket-version"), "{webtunnel}");
        assert_eq!(check_upgrade_or_raw(&keyless), Ok(Upgrade::Raw));
        assert_eq!(check_upgrade(&keyless).unwrap_err().status, 426, "the framed check is unchanged");
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(check_upgrade_or_raw(&head("")), Ok(Upgrade::Frames(key.to_string())));
        let status = |text: String| check_upgrade_or_raw(&parse(&text)).unwrap_err().status;
        let base = webtunnel.trim_end_matches("\r\n").to_string();
        assert_eq!(status(format!("{base}\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: c2hvcnQ=\r\n\r\n")), 400);
        assert_eq!(status(format!("{base}\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key:\r\n\r\n")), 400);
        assert_eq!(status(format!("{base}\r\nSec-WebSocket-Key: {key}\r\n\r\n")), 426, "a key takes the version");
        assert_eq!(status(format!("{base}\r\nOrigin: https://page.example\r\n\r\n")), 403);
        assert_eq!(check_upgrade_or_raw(&head("Origin: null\r\n")).unwrap_err().status, 403);
        assert_eq!(status(webtunnel.replacen("GET ", "POST ", 1)), 405);
        assert_eq!(status(webtunnel.replace("Upgrade: websocket\r\n", "")), 426);
        assert_eq!(status(webtunnel.replace("Host: bridge.example\r\n", "")), 400);
        let answer = String::from_utf8(switching_raw()).unwrap();
        assert_eq!(answer, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n");
        assert_eq!(webtunnel_takes(&answer), Ok(()), "a WebTunnel client takes it");
    }
}
