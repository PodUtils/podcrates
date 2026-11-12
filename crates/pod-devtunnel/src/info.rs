//! The connect information of a tunnel: the answer of the management API
//! to a get of the tunnel with the scope `connect`. It gives the ids of the
//! tunnel, the relay endpoint of each host (its client relay URI and its
//! host keys) and the connect token. The bytes come from the network:
//! [`ConnectInfo::parse`] bounds each part, and its failure quotes no byte
//! of the answer, which holds a token.

use std::collections::HashMap;
use std::fmt;

use pod_core::{Class, Failure};
use pod_ws::url::WsUrl;
use serde::Deserialize;

use crate::failure::{Step, fail};
use crate::settings::is_loopback;
use crate::token::ConnectToken;

/// The most bytes of an answer of the management API.
pub const ANSWER_MOST: usize = 1024 * 1024;
/// The most endpoints and host keys that the client reads.
pub const ENDPOINTS_MOST: usize = 64;
pub const KEYS_MOST: usize = 16;
/// The connection mode of a relay endpoint.
const RELAY_MODE: &str = "TunnelRelay";

/// The ids of a tunnel. None of them is a secret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tunnel {
    pub cluster_id: Option<String>,
    pub tunnel_id: Option<String>,
    pub name: Option<String>,
}

impl Tunnel {
    /// The name, else `ID.CLUSTER`, else `ID`, for a line.
    pub fn shown(&self) -> String {
        match (&self.name, &self.tunnel_id, &self.cluster_id) {
            (Some(name), _, _) => name.clone(),
            (None, Some(id), Some(cluster)) => format!("{id}.{cluster}"),
            (None, Some(id), None) => id.clone(),
            (None, None, _) => "a tunnel with no name and no id".into(),
        }
    }
}

/// A relay endpoint of a host of the tunnel.
#[derive(Clone)]
pub struct Endpoint {
    /// The id of the host.
    pub host_id: String,
    /// The client relay URI: `wss://`, or `ws://` to a loopback host.
    pub relay: WsUrl,
    /// The host keys, each the base64 of its SSH wire form, as
    /// `hostPublicKeys` gives them. The host of the SSH session must have
    /// one of them.
    pub host_keys: Vec<String>,
}

impl fmt::Debug for Endpoint {
    /// The relay URI with no query, which could hold a secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("host_id", &self.host_id)
            .field("relay", &self.relay.shown())
            .field("host_keys", &self.host_keys.len())
            .finish()
    }
}

/// The connect information of a tunnel.
#[derive(Clone)]
pub struct ConnectInfo {
    pub tunnel: Tunnel,
    /// The relay endpoints, in the order of the answer.
    pub endpoints: Vec<Endpoint>,
    token: ConnectToken,
}

impl fmt::Debug for ConnectInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectInfo")
            .field("tunnel", &self.tunnel)
            .field("endpoints", &self.endpoints)
            .field("token", &self.token)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTunnel {
    cluster_id: Option<String>,
    tunnel_id: Option<String>,
    name: Option<String>,
    access_tokens: Option<HashMap<String, String>>,
    #[serde(default)]
    endpoints: Vec<RawEndpoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEndpoint {
    connection_mode: Option<String>,
    host_id: Option<String>,
    #[serde(default)]
    host_public_keys: Vec<String>,
    client_relay_uri: Option<String>,
}

/// An id of the answer: at most 128 bytes of letters, digits, `-`, `_`
/// and `.`, as a line may show it.
fn id(value: Option<String>) -> Option<String> {
    value.filter(|v| {
        !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    })
}

/// A host key: base64 of at most 4 KiB.
fn key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
}

impl ConnectInfo {
    /// The connect information of `body`, an answer of the management API
    /// to a get with the token scope `scope`. Each relay endpoint with a
    /// client relay URI that parses is kept; a `ws://` URI to a host that
    /// is not the loopback is not. A failure is of the step
    /// [`Step::Management`]: a body that is not the JSON of a tunnel is a
    /// protocol failure, no connect token is a refusal (the user has no
    /// connect access), no relay endpoint is an unavailable tunnel (no host
    /// is connected).
    pub fn parse(body: &[u8], scope: &str) -> Result<ConnectInfo, Failure> {
        let protocol = |what: String| fail(Step::Management, Class::Protocol, what);
        if body.len() > ANSWER_MOST {
            return Err(protocol(format!("the answer is longer than {ANSWER_MOST} bytes")));
        }
        // The words of serde_json can quote a value of the answer: only the
        // category and the place of the fault are kept.
        let raw: RawTunnel = serde_json::from_slice(body).map_err(|e| {
            protocol(format!(
                "the answer is not the JSON of a tunnel ({:?} at line {}, column {})",
                e.classify(),
                e.line(),
                e.column()
            ))
        })?;
        let tunnel = Tunnel { cluster_id: id(raw.cluster_id), tunnel_id: id(raw.tunnel_id), name: id(raw.name) };
        let mut tokens = raw.access_tokens.unwrap_or_default();
        let Some(token) = tokens.remove(scope) else {
            return Err(fail(
                Step::Management,
                Class::Refused,
                format!("the answer for {} holds no token of the scope {scope:?}", tunnel.shown()),
            ));
        };
        let token = ConnectToken::new(token)?;
        let mut endpoints = Vec::new();
        for raw in raw.endpoints.into_iter().take(ENDPOINTS_MOST) {
            if raw.connection_mode.as_deref() != Some(RELAY_MODE) {
                continue;
            }
            let Some(relay) = raw.client_relay_uri.as_deref().and_then(|u| WsUrl::parse(u).ok()) else {
                continue;
            };
            if !relay.tls && !is_loopback(&relay.host) {
                tracing::warn!(relay = %relay.shown(), "a relay endpoint with no TLS to a host that is not the loopback");
                continue;
            }
            let host_keys: Vec<String> = raw.host_public_keys.into_iter().filter(|k| key(k)).take(KEYS_MOST).collect();
            let host_id = id(raw.host_id).unwrap_or_default();
            endpoints.push(Endpoint { host_id, relay, host_keys });
        }
        if endpoints.is_empty() {
            return Err(fail(
                Step::Management,
                Class::Unavailable,
                format!("{} has no relay endpoint: no host of the tunnel is connected", tunnel.shown()),
            ));
        }
        Ok(ConnectInfo { tunnel, endpoints, token })
    }

    /// The connect token.
    pub fn token(&self) -> &ConnectToken {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"clusterId":"usw2","tunnelId":"bcd123","name":"my-box",
        "accessTokens":{"connect":"made-up-connect-token","manage":"other"},
        "endpoints":[
          {"connectionMode":"LocalNetwork","hostId":"h0","hostPublicKeys":["AAAA"]},
          {"connectionMode":"TunnelRelay","hostId":"h1","hostPublicKeys":["AAAAC3Nza=", "bad key!"],
           "clientRelayUri":"wss://usw2-data.rel.tunnels.api.visualstudio.com/api/v1/Client/Connect/bcd123?x=secret"},
          {"connectionMode":"TunnelRelay","hostId":"h2","clientRelayUri":"ws://relay.example/c"},
          {"connectionMode":"TunnelRelay","hostId":"h3","clientRelayUri":"ws://127.0.0.1:9/c"}
        ]}"#;

    #[test]
    fn the_connect_information_keeps_each_relay_endpoint_that_a_client_can_use() {
        let info = ConnectInfo::parse(GOOD.as_bytes(), "connect").unwrap();
        assert_eq!(info.tunnel.shown(), "my-box");
        assert_eq!(info.tunnel.cluster_id.as_deref(), Some("usw2"));
        let hosts: Vec<&str> = info.endpoints.iter().map(|e| e.host_id.as_str()).collect();
        assert_eq!(hosts, ["h1", "h3"]);
        assert_eq!(info.endpoints[0].host_keys, ["AAAAC3Nza="]);
        assert!(info.endpoints[0].relay.tls);
        let shown = format!("{info:?}");
        assert!(!shown.contains("made-up-connect-token") && !shown.contains("secret"), "{shown}");
    }

    #[test]
    fn an_answer_with_no_token_or_no_relay_is_named() {
        let no_token = r#"{"name":"a","endpoints":[]}"#;
        assert_eq!(ConnectInfo::parse(no_token.as_bytes(), "connect").unwrap_err().class, Class::Refused);
        let no_relay = r#"{"name":"a","accessTokens":{"connect":"t"},"endpoints":[]}"#;
        let failure = ConnectInfo::parse(no_relay.as_bytes(), "connect").unwrap_err();
        assert_eq!(failure.class, Class::Unavailable);
        assert!(failure.chain().contains("a has no relay endpoint"), "{}", failure.chain());
    }

    #[test]
    fn a_body_that_is_no_tunnel_is_a_protocol_failure_that_quotes_nothing() {
        let leaky = r#"{"name":"a","accessTokens":{"connect":["made-up-connect-token"]}}"#;
        for body in [leaky, "", "[]", "{\"endpoints\": 5}"] {
            let failure = ConnectInfo::parse(body.as_bytes(), "connect").unwrap_err();
            assert_eq!(failure.class, Class::Protocol, "{body}");
            assert!(!failure.chain().contains("made-up"), "{}", failure.chain());
        }
        let token = r#"{"accessTokens":{"connect":"two words"},"endpoints":[]}"#;
        let failure = ConnectInfo::parse(token.as_bytes(), "connect").unwrap_err();
        assert!(!failure.chain().contains("two words"), "{}", failure.chain());
    }
}
