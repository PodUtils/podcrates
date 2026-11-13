//! The URL of the carriers over HTTPS (`h2`, `poll`): `https://HOST[:PORT]/PATH`,
//! and the dial address `KEY@URL` with the key of the listener.

use std::net::{IpAddr, SocketAddr};

use pod_core::Target;
use pod_identity::KeyName;

use crate::carrier::{CarrierError, CarrierErrorKind};

/// The URL of a request: `https://HOST[:PORT]/PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostUrl {
    /// A name or an IP address, an IPv6 one with no brackets.
    pub host: String,
    pub port: u16,
    /// The path and its query, from `/`.
    pub path: String,
}

impl PostUrl {
    /// `https://HOST[:PORT]/PATH`: the rules of a WebSocket's URL
    /// (`pod_ws::url::WsUrl`), with the scheme `https`.
    pub fn parse(text: &str) -> Result<PostUrl, String> {
        let rest = match text.get(..8) {
            Some(scheme) if scheme.eq_ignore_ascii_case("https://") => &text[8..],
            _ => return Err("not an URL of HTTPS, which is https://HOST[:PORT]/PATH".into()),
        };
        let url = pod_ws::url::WsUrl::parse(&format!("wss://{rest}"))?;
        Ok(PostUrl { host: url.host, port: url.port, path: url.target })
    }

    /// The `:authority` or the `Host` of a request: the port only when it
    /// is not 443.
    pub fn authority(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == 443 { host } else { format!("{host}:{}", self.port) }
    }

    /// The target of the dial.
    pub fn target(&self) -> Target {
        match self.host.parse::<IpAddr>() {
            Ok(ip) => Target::Addr(SocketAddr::new(ip, self.port)),
            Err(_) => Target::Name { host: self.host.clone(), port: self.port },
        }
    }
}

/// The key of the listener and its URL in a dial address
/// `KEY@https://HOST[:PORT]/PATH`.
pub(crate) fn address(carrier: &'static str, address: &str) -> Result<(KeyName, PostUrl), CarrierError> {
    let usage = |text: String| CarrierError::new(carrier, CarrierErrorKind::Usage, text);
    let (key, url) = address
        .split_once('@')
        .ok_or_else(|| usage(format!("{address:?}: give KEY@https://HOST:PORT/PATH, with the key of the listener")))?;
    let key = KeyName::parse(key).ok_or_else(|| {
        usage(format!("{key:?} is no key: give the key in hex or base32, its SHA256: fingerprint or its display form"))
    })?;
    let url = PostUrl::parse(url).map_err(usage)?;
    Ok((key, url))
}
