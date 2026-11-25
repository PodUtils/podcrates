//! Configuration: credentials, transport, ingress and reconnect policy.

mod file;
pub use file::*;

mod keys;
use keys::nearest_key;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fail::{Context, Failure, IoFail, OrFail, Result, bail};
use base64::Engine;
use pod_core::{Class, Stage};
use serde::Deserialize;
use uuid::Uuid;

use crate::ingress::{Ingress, Service};

/// How to authenticate the tunnel with the edge. `Debug` hides the token and
/// the secret.
#[derive(Clone)]
pub enum Credentials {
    /// Anonymous `trycloudflare.com` quick tunnel (provisioned on start).
    Quick,
    /// Named tunnel token: base64 of `{"a":account,"t":tunnel_id,"s":secret}`.
    Token(String),
    /// A cloudflared credentials JSON file.
    File(PathBuf),
    /// The three fields already decoded.
    Explicit { account_tag: String, tunnel_id: Uuid, secret: Vec<u8> },
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::Quick => f.write_str("Quick"),
            Credentials::Token(_) => f.write_str("Token(<hidden>)"),
            Credentials::File(path) => f.debug_tuple("File").field(path).finish(),
            Credentials::Explicit { account_tag, tunnel_id, .. } => f
                .debug_struct("Explicit")
                .field("account_tag", account_tag)
                .field("tunnel_id", tunnel_id)
                .field("secret", &"<hidden>")
                .finish(),
        }
    }
}

#[derive(Deserialize)]
struct TokenJson {
    a: String,
    t: String,
    s: String,
}

#[derive(Deserialize)]
struct CredentialsJson {
    #[serde(rename = "AccountTag", alias = "account_tag")]
    account_tag: String,
    #[serde(rename = "TunnelID", alias = "tunnel_id")]
    tunnel_id: String,
    #[serde(rename = "TunnelSecret", alias = "tunnel_secret")]
    tunnel_secret: String,
}

/// Decode a named-tunnel token. Cloudflare gives the JSON envelope in standard
/// base64. The secret in the envelope is also base64.
pub fn parse_token(token: &str) -> Result<Credentials> {
    let token = token.trim();
    let raw = decode_b64(token).context("token is not valid base64")?;
    let parsed: TokenJson = serde_json::from_slice(&raw).or_fail(Stage::Host, Class::Config, "the token payload")?;
    let secret = decode_b64(&parsed.s)?;
    let tunnel_id = Uuid::parse_str(&parsed.t).or_fail(Stage::Host, Class::Config, "token tunnel id is not a UUID")?;
    Ok(Credentials::Explicit { account_tag: parsed.a, tunnel_id, secret })
}

/// Load a cloudflared credentials file.
pub fn parse_credentials_file(path: &std::path::Path) -> Result<Credentials> {
    let text =
        std::fs::read_to_string(path).io_fail(Stage::Host, format!("reading credentials file {}", path.display()))?;
    // The error of serde names a missing field, or the place of bad JSON.
    let parsed: CredentialsJson = serde_json::from_str(&text).or_fail(
        Stage::Host,
        Class::Config,
        format!("credentials file {}", path.display()),
    )?;
    let secret = decode_b64(&parsed.tunnel_secret)
        .with_context(|| format!("credentials file {}: TunnelSecret is not base64", path.display()))?;
    let tunnel_id = Uuid::parse_str(&parsed.tunnel_id).or_fail(
        Stage::Host,
        Class::Config,
        format!("credentials file {}: TunnelID is not a UUID", path.display()),
    )?;
    Ok(Credentials::Explicit { account_tag: parsed.account_tag, tunnel_id, secret })
}

fn decode_b64(value: &str) -> Result<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
    STANDARD.decode(value).or_else(|_| URL_SAFE.decode(value)).or_else(|_| URL_SAFE_NO_PAD.decode(value)).or_fail(
        Stage::Host,
        Class::Config,
        "base64",
    )
}

/// The edge protocol (`--protocol`): `auto`, `quic` or `http2`. The leg plan
/// of the tunnel reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Protocol {
    #[default]
    Auto,
    Quic,
    Http2,
}

impl Protocol {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Protocol::Auto),
            "quic" => Ok(Protocol::Quic),
            "http2" | "h2" => Ok(Protocol::Http2),
            other => bail!(Stage::Host, Class::Config, "unknown protocol {other:?}; expected auto, quic or http2"),
        }
    }

    /// The name, as cloudflared writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Auto => "auto",
            Protocol::Quic => "quic",
            Protocol::Http2 => "http2",
        }
    }
}

/// The carrier of the connection to the tunnel edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Transport {
    /// Dial the edge directly on port 7844. This is fast, and `cloudflared`
    /// does it.
    Direct,
    /// Carry the edge connection in a WARP MASQUE tunnel on TCP 443. Then the
    /// host does not connect to port 7844.
    Warp,
    /// Decide from the host: direct when the edge answers on 7844, else WARP.
    /// This is the default. Thus a quick tunnel works on an egress that
    /// permits only 443, and the caller does not have to know the network.
    #[default]
    Auto,
    /// Carry the HTTP/2 leg in a WebSocket to a relay on TCP 443. The relay
    /// opens TCP to the edge ([`crate::host::EdgeRelay`]). For a host whose
    /// proxy carries only port 443 and no WARP.
    Relay,
}

impl Transport {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Transport::Auto),
            "direct" => Ok(Transport::Direct),
            "warp" | "masque" | "warp+masque" => Ok(Transport::Warp),
            "relay" => Ok(Transport::Relay),
            other => {
                bail!(Stage::Host, Class::Config, "unknown transport {other:?}; expected auto, direct, warp or relay")
            }
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Transport::Auto => "auto",
            Transport::Direct => "direct",
            Transport::Warp => "warp",
            Transport::Relay => "relay",
        })
    }
}

/// What a leg does when the edge drops a connection.
///
/// A leg tries again after each network error, with no limit by default:
/// after a network loss of any length, the tunnel serves again. The delay
/// doubles from `initial` to `max`, plus 0 to 25 % at random, so that the
/// legs do not reconnect at the same time. A refusal of the edge with
/// `should_retry = false` stops the leg; its `retry_after` replaces the
/// delay. The count of attempts starts again when a connection stays up
/// for `stable`.
#[derive(Clone, Debug)]
pub struct ReconnectPolicy {
    pub max_attempts: u32,
    pub initial: Duration,
    pub max: Duration,
    pub stable: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: u32::MAX,
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
            stable: Duration::from_secs(60),
        }
    }
}

/// What a leg does after a failed attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Try again after this delay.
    Retry(Duration),
    /// Stop the leg, for this reason.
    Stop(String),
}

/// The longest `retry_after` that the client obeys.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);

impl ReconnectPolicy {
    /// The delay before attempt `attempt` (from 1), with no jitter.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(20);
        let secs = self.initial.as_secs().saturating_mul(1u64 << shift);
        Duration::from_secs(secs.min(self.max.as_secs()))
    }

    /// The delay before attempt `attempt`, plus 0 to 25 % at random.
    pub fn backoff_with_jitter(&self, attempt: u32) -> Duration {
        let base = self.backoff(attempt);
        // A failure of the system source gives no jitter.
        let mut byte = [0u8; 2];
        let _ = pod_core::entropy::Entropy::fill(&mut pod_core::entropy::OsEntropy, &mut byte);
        let fraction = u16::from_le_bytes(byte) as u32 % 2501; // 0..=25.00 %
        base + base * fraction / 10_000
    }

    /// What to do after attempt `attempt` failed with `err`.
    pub fn next(&self, attempt: u32, err: &Failure) -> Next {
        if let Some(refused) = crate::fail::find::<crate::cloudflare::rpc::Refused>(err) {
            if !refused.should_retry {
                return Next::Stop(format!(
                    "the edge refused the registration and asks for no retry: {}",
                    refused.cause
                ));
            }
            if !refused.retry_after.is_zero() {
                return Next::Retry(refused.retry_after.clamp(Duration::from_secs(1), MAX_RETRY_AFTER));
            }
        }
        if attempt >= self.max_attempts {
            return Next::Stop(format!("{attempt} attempts failed; the last: {}", err.chain()));
        }
        Next::Retry(self.backoff_with_jitter(attempt + 1))
    }
}

/// All that is necessary to run a tunnel.
#[derive(Clone, Debug)]
pub struct Config {
    pub service_url: String,
    pub user_agent: String,
    pub credentials: Credentials,
    pub protocol: Protocol,
    pub transport: Transport,
    /// The edge connections of the tunnel, 1 to 8; the default is 2 for each
    /// tunnel, a quick tunnel too: when one connection is lost, the other
    /// carries the visitors. cloudflared opens one for a quick tunnel; set 1
    /// for its behaviour (podnet's choice, PodUtils/podnet#654).
    pub ha_connections: u8,
    pub reconnect: ReconnectPolicy,
    pub ingress: Ingress,
    pub verify: bool,
    /// How the next edge leg starts (`--leg-failover`).
    pub failover: crate::race::Failover,
    /// The secret texts of the configuration file (its `token:`): the
    /// consumer hides them in its log and its reports.
    pub secrets: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            service_url: crate::services::default_quick_service(),
            user_agent: concat!("pod-cloudflare/", env!("CARGO_PKG_VERSION")).to_string(),
            credentials: Credentials::Quick,
            protocol: Protocol::Auto,
            transport: Transport::default(),
            ha_connections: 2,
            reconnect: ReconnectPolicy::default(),
            ingress: Ingress::default(),
            verify: false,
            failover: crate::race::Failover::Staggered(crate::race::LEG_STAGGER),
            secrets: Vec::new(),
        }
    }
}

impl Config {
    /// One catch-all HTTP service.
    pub fn with_service(service: Service) -> Self {
        Config { ingress: Ingress::default().catch_all(service), ..Config::default() }
    }
}

#[cfg(test)]
mod tests;
