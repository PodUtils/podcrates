//! An Outline access key's body: what an `ssconf://` URL serves, read
//! as Outline's client reads it (developer.getoutline.org, "Dynamic Access
//! Keys" and "Access Key Configuration"):
//!
//! - an `ss://` link, one line;
//! - JSON with `server`, `server_port`, `password` and `method`, as SIP008's
//!   server is;
//! - YAML (or JSON) with `transport`: `$type: tcpudp` and its `tcp` part, or
//!   the part itself, `$type: shadowsocks` with `endpoint`, `cipher` and
//!   `secret`. The endpoint is `HOST:PORT` (raw TCP), `$type: websocket`
//!   with a `wss://` URL (Shadowsocks in a WebSocket), or
//!   `$type: first-supported` with its `options`, of which this client takes the
//!   first that it reads.
//!
//! The UDP part is not read: this client carries TCP. A salt's `prefix`
//! (`super::prefix`) goes first in each stream's salt. A WebSocket's own
//! `endpoint` (a dial to another address than its URL's) and the other
//! types of Outline's configuration are refused by name. [`key_file`] reads
//! the key from a private file, so that a WebSocket's secret path stays out
//! of a command line. A key holds a password: no message here quotes it.

use std::path::Path;

use serde_json::Value;

use super::uri::authority_of;
use super::{Method, Node};

/// The most bytes of a key file: a key is some lines.
const MAX_KEY_FILE: u64 = 64 * 1024;

/// The server of the access key in the private file at `path`: a regular
/// file of this user that nobody else can read, of 64 KiB at most, read as
/// [`access_key`] reads a body. A failure names the file and never its text.
pub fn key_file(path: &Path) -> Result<Node, String> {
    let text = zeroize::Zeroizing::new(pod_core::files::read_private_up_to(path, MAX_KEY_FILE)?);
    access_key(text.as_bytes()).map_err(|why| format!("{}: {why}", path.display()))
}

/// The server of an access key's `body`.
pub fn access_key(body: &[u8]) -> Result<Node, String> {
    let text = String::from_utf8_lossy(body);
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("the access key is empty".into());
    }
    if text.get(..5).is_some_and(|scheme| scheme.eq_ignore_ascii_case("ss://")) {
        return Node::parse(text.lines().next().unwrap_or_default());
    }
    let doc = if text.starts_with('{') {
        serde_json::from_str(text).map_err(|_| "the access key starts with { and is not JSON".to_string())?
    } else {
        super::yaml::parse(text).map_err(|why| format!("the access key's YAML: {why}"))?
    };
    match doc.get("transport") {
        Some(transport) => self::transport(transport),
        None if doc.get("server").is_some() => with_prefix(&doc, super::list::sip008_node(&doc)?),
        None => Err("the access key names neither a transport nor a server".into()),
    }
}

/// `$type` of a part of the configuration.
fn kind(part: &Value) -> Option<&str> {
    part.get("$type").and_then(Value::as_str)
}

/// The TCP part of a transport: `tcpudp`'s `tcp`, or a transport of one.
fn transport(transport: &Value) -> Result<Node, String> {
    let tcp = match kind(transport) {
        Some("tcpudp") => transport.get("tcp").ok_or("its transport tcpudp has no tcp part")?,
        Some("shadowsocks") => transport,
        Some(other) => return Err(format!("its transport is of the type {other}, which this client does not speak")),
        None => return Err("its transport has no $type".into()),
    };
    match kind(tcp) {
        Some("shadowsocks") => {}
        Some(other) => return Err(format!("its TCP part is of the type {other}, which this client does not speak")),
        None => return Err("its TCP part has no $type".into()),
    }
    let text = |key: &str| tcp.get(key).and_then(Value::as_str);
    let method = Method::parse(text("cipher").ok_or("its TCP part has no cipher")?)?;
    let secret = text("secret").ok_or("its TCP part has no secret")?;
    let node = endpoint(tcp.get("endpoint").ok_or("its TCP part has no endpoint")?, method, secret)?;
    with_prefix(tcp, node)
}

/// The server of an endpoint, for `method` and `secret`.
fn endpoint(endpoint: &Value, method: Method, secret: &str) -> Result<Node, String> {
    if let Some(at) = endpoint.as_str() {
        let (host, port) = authority_of(at.trim()).map_err(|why| format!("its endpoint: {why}"))?;
        return Node::new(method, secret, &host, port);
    }
    match kind(endpoint) {
        Some("websocket") => {
            if endpoint.get("endpoint").is_some() {
                return Err(
                    "its WebSocket names an endpoint of its own (a dial to another address than its URL's), which \
                     this client does not take"
                        .into(),
                );
            }
            let url = endpoint.get("url").and_then(Value::as_str).ok_or("its WebSocket endpoint has no url")?;
            // The URL's path is often the server's secret: never quoted.
            let url = pod_ws::url::WsUrl::parse(url).map_err(|_| "its WebSocket's url is not a ws:// or wss:// URL")?;
            Node::over_websocket(method, secret, url)
        }
        Some("first-supported") => {
            let options = endpoint.get("options").and_then(Value::as_array);
            let options = options.ok_or("its first-supported has no options")?;
            let mut whys = Vec::new();
            for (i, option) in options.iter().enumerate() {
                match self::endpoint(option, method, secret) {
                    Ok(node) => return Ok(node),
                    Err(why) => whys.push(format!("option {}: {why}", i + 1)),
                }
            }
            Err(format!("no option of its first-supported is one that this client takes ({})", whys.join("; ")))
        }
        Some(other) => Err(format!("its endpoint is of the type {other}, which this client does not speak")),
        None => Err("its endpoint is neither HOST:PORT nor a part with a $type".into()),
    }
}

/// `node` with the salt's prefix of `part`, which makes the stream's first
/// bytes look like another protocol's.
fn with_prefix(part: &Value, node: Node) -> Result<Node, String> {
    match part.get("prefix") {
        None | Some(Value::Null) => Ok(node),
        Some(Value::String(text)) => node.with_prefix(&super::prefix::from_text(text)?),
        Some(_) => Err("its prefix is not a string".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_form_of_outline_s_documents_gives_its_server() {
        let json = br#"{"server":"ss.example","server_port":8388,"password":"pw","method":"chacha20-ietf-poly1305"}"#;
        let raw = access_key(json).unwrap();
        assert_eq!((raw.host.as_str(), raw.port, raw.ws.is_none()), ("ss.example", 8388, true));
        let yaml = "transport:\n  $type: tcpudp\n  tcp:\n    $type: shadowsocks\n    endpoint: ss.example:8388\n    \
                    cipher: aes-256-gcm\n    secret: pw\n";
        let tcp = access_key(yaml.as_bytes()).unwrap();
        assert_eq!((tcp.method, tcp.port), (Method::Aes256Gcm, 8388));
        let ws = "transport:\n  $type: shadowsocks\n  endpoint:\n    $type: websocket\n    \
                  url: wss://ss.example/SECRET/tcp\n  \
                  cipher: chacha20-ietf-poly1305\n  secret: pw\n";
        let node = access_key(ws.as_bytes()).unwrap();
        assert_eq!(node.ws.as_ref().map(|u| u.target.as_str()), Some("/SECRET/tcp"));
        assert_eq!((node.host.as_str(), node.port), ("ss.example", 443));
        assert_eq!(node.shown(), "wss://ss.example/...");
        assert!(!format!("{node:?}").contains("SECRET") && !format!("{node:?}").contains("pw\""));
        let first = "transport:\n  $type: shadowsocks\n  endpoint:\n    $type: first-supported\n    options:\n      \
                     - $type: shadowsocks-over-quic\n      - $type: websocket\n        url: wss://b.example/p\n      \
                     - ss.example:4321\n  cipher: aes-128-gcm\n  secret: pw\n";
        assert_eq!(access_key(first.as_bytes()).unwrap().host, "b.example", "the first option that this client takes");
        let link = access_key(b"ss://YWVzLTEyOC1nY206dGVzdA@192.168.100.1:8888/?outline=1\n").unwrap();
        assert_eq!(link.port, 8888);
    }

    #[test]
    fn what_podssh_does_not_take_is_named_and_no_secret_is_quoted() {
        let refused = |text: &str| access_key(text.as_bytes()).unwrap_err();
        let plain_ws = "transport:\n  $type: shadowsocks\n  endpoint:\n    $type: websocket\n    \
                        url: ws://ss.example/p\n  \
                        cipher: aes-128-gcm\n  secret: hunter2\n";
        let why = refused(plain_ws);
        assert!(why.contains("wss://") && !why.contains("hunter2"), "{why}");
        let prefix = concat!(
            r#"{"server":"s.example","server_port":1,"password":"hunter2","method":"aes-128-gcm","#,
            r#""prefix":"\u0016\u0003\u0001\u0000\u00a8\u0001\u0001\u0000\u0000"}"#
        );
        let long = refused(prefix);
        assert!(long.contains("9 bytes; with aes-128-gcm it has 8 at most") && !long.contains("hunter2"), "{long}");
        assert!(refused("transport:\n  $type: tcpudp\n  tcp:\n    $type: vless\n").contains("type vless"));
        let own = "transport:\n  $type: shadowsocks\n  endpoint:\n    $type: websocket\n    \
                   url: wss://a.example/p\n    \
                   endpoint: cloudflare.net:443\n  cipher: aes-128-gcm\n  secret: hunter2\n";
        assert!(refused(own).contains("endpoint of its own"));
        assert!(refused("hello: world\n").contains("neither a transport nor a server"));
        assert!(refused("").contains("empty"));
    }
}
