//! The device record of the WARP API: its id, its token, its account, its
//! addresses in the tunnel and the peer of its front door.
//!
//! `Serialize` is not for the wire (the API takes only the bodies of
//! [`crate::register`]): a caller keeps a registered device and resumes it
//! with no second registration. `Debug` hides the token and the license.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use serde::{Deserialize, Serialize};

use crate::service;

/// The device as the API reports it.
#[derive(Clone, Deserialize, Serialize)]
pub struct Device {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub key_type: String,
    #[serde(default)]
    pub tunnel_type: String,
    pub account: Account,
    pub config: DeviceConfig,
    /// Only the answer of `POST /reg` has it.
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub warp_enabled: bool,
    #[serde(default)]
    pub waitlist_enabled: bool,
    #[serde(default)]
    pub policy: Policy,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key", &self.key)
            .field("key_type", &self.key_type)
            .field("tunnel_type", &self.tunnel_type)
            .field("account", &self.account)
            .field("config", &self.config)
            .field("token", &"<hidden>")
            .field("warp_enabled", &self.warp_enabled)
            .field("waitlist_enabled", &self.waitlist_enabled)
            .field("policy", &self.policy)
            .finish()
    }
}

/// The account of a device. `Debug` hides the license key.
#[derive(Clone, Deserialize, Serialize)]
pub struct Account {
    pub id: String,
    #[serde(default)]
    pub account_type: String,
    #[serde(default)]
    pub license: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("id", &self.id)
            .field("account_type", &self.account_type)
            .field("license", &"<hidden>")
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub interface: Interface,
    #[serde(default)]
    pub services: Services,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Peer {
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub endpoint: Endpoint,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Endpoint {
    #[serde(default)]
    pub v4: String,
    #[serde(default)]
    pub v6: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Interface {
    #[serde(default)]
    pub addresses: Addresses,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Addresses {
    #[serde(default)]
    pub v4: String,
    #[serde(default)]
    pub v6: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Services {
    #[serde(default)]
    pub http_proxy: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Policy {
    #[serde(default)]
    pub tunnel_protocol: String,
    #[serde(default)]
    pub post_quantum: String,
}

/// The address of `raw` with no prefix length.
fn address<T: std::str::FromStr>(raw: &str) -> Option<T> {
    let raw = raw.trim();
    raw.split('/').next().unwrap_or(raw).parse().ok()
}

impl Device {
    /// The IPv4 address that the tunnel assigned, with no prefix length.
    pub fn ipv4(&self) -> Option<Ipv4Addr> {
        address(&self.config.interface.addresses.v4)
    }

    /// The IPv6 address that the tunnel assigned, with no prefix length.
    pub fn ipv6(&self) -> Option<Ipv6Addr> {
        address(&self.config.interface.addresses.v6)
    }

    /// The address of the device in the tunnel: IPv4 first.
    pub fn local_address(&self) -> Option<IpAddr> {
        self.ipv4().map(IpAddr::V4).or_else(|| self.ipv6().map(IpAddr::V6))
    }

    /// The MASQUE front door, as an address to dial: the first of
    /// [`Device::masque_endpoints`].
    pub fn masque_endpoint(&self) -> Option<SocketAddr> {
        self.masque_endpoints().first().copied()
    }

    /// The addresses of the MASQUE front door to try, in order
    /// (PodUtils/podnet issue 124): the IPv4 address of the first peer, then
    /// its IPv6 address, each on [`service::masque_port`]. Both have the key
    /// of the first peer; the other peers have other keys.
    pub fn masque_endpoints(&self) -> Vec<SocketAddr> {
        let Some(peer) = self.config.peers.first() else {
            return Vec::new();
        };
        if self.config.peers.len() > 1 {
            tracing::debug!(peers = self.config.peers.len(), "the device record has more than one peer");
        }
        let port = service::masque_port();
        let mut out = Vec::new();
        let v4 = peer.endpoint.v4.split(':').next().unwrap_or("").trim();
        if let Ok(ip) = v4.parse::<Ipv4Addr>() {
            out.push(SocketAddr::new(ip.into(), port));
        }
        let v6 = peer.endpoint.v6.trim().trim_start_matches('[');
        let v6 = v6.split(']').next().unwrap_or(v6);
        if let Ok(ip) = v6.parse::<Ipv6Addr>() {
            out.push(SocketAddr::new(ip.into(), port));
        }
        out
    }

    /// The pinned public key of the front door, PEM (`PUBLIC KEY`).
    pub fn peer_public_key_pem(&self) -> Option<&str> {
        self.config.peers.first().map(|p| p.public_key.as_str()).filter(|k| k.contains("BEGIN PUBLIC KEY"))
    }

    /// The three bytes of the WireGuard "reserved" tag: the first three
    /// bytes of the base64 `client_id`.
    pub fn reserved(&self) -> Option<[u8; 3]> {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::STANDARD.decode(self.config.client_id.as_bytes()).ok()?;
        (raw.len() >= 3).then(|| [raw[0], raw[1], raw[2]])
    }
}

/// Hide the license key in an account record (`GET /reg/{id}/account`):
/// each key named `license`, at any depth, with a string value that is not
/// empty becomes `"<hidden>"`.
pub fn redact_account(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if key.eq_ignore_ascii_case("license") {
                    if item.as_str().is_some_and(|s| !s.is_empty()) {
                        *item = serde_json::Value::String("<hidden>".into());
                    }
                } else {
                    redact_account(item);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_account),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_license_key_is_hidden_in_the_account_record() {
        let mut account = serde_json::json!({
            "account_type": "limited",
            "license": "abcd1234-efgh5678-ijkl9012",
            "nested": [{"license": "x1y2z3"}],
            "empty": {"license": ""},
            "warp_plus": true
        });
        redact_account(&mut account);
        let text = account.to_string();
        assert!(!text.contains("abcd1234") && !text.contains("x1y2z3"), "{text}");
        assert_eq!(account["license"], "<hidden>");
        assert_eq!(account["nested"][0]["license"], "<hidden>");
        assert_eq!(account["empty"]["license"], "");
        assert_eq!(account["account_type"], "limited");
    }

    #[test]
    fn endpoint_parsing_strips_the_zero_port() {
        let device: Device = serde_json::from_str(
            r#"{"id":"x","account":{"id":"a"},"config":{"peers":[{"public_key":"k",
                "endpoint":{"v4":"162.159.198.2:0","v6":"[2606:4700:103::2]:0"}}],
                "interface":{"addresses":{"v4":"172.16.0.2","v6":"2606:4700:110::1"}}}}"#,
        )
        .unwrap();
        assert_eq!(device.masque_endpoint(), Some("162.159.198.2:443".parse().unwrap()));
        // PodUtils/podnet issue 124: both addresses of the peer, IPv4 first.
        assert_eq!(
            device.masque_endpoints(),
            ["162.159.198.2:443".parse().unwrap(), "[2606:4700:103::2]:443".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(device.ipv4(), Some("172.16.0.2".parse().unwrap()));
        assert_eq!(device.ipv6(), Some("2606:4700:110::1".parse().unwrap()));
        assert_eq!(device.local_address(), Some("172.16.0.2".parse().unwrap()));
        let shown = format!("{device:?}");
        assert!(shown.contains("<hidden>"), "{shown}");
    }
}
