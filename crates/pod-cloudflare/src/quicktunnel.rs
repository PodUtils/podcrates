//! Provision an anonymous Cloudflare "quick tunnel", as `cloudflared` does.
//!
//! # What this module does again
//!
//! `cloudflared tunnel --url ...` sends `POST https://api.trycloudflare.com/tunnel`
//! with an empty body and **no credentials**. The response has a tunnel id, an
//! account tag, a 32-byte secret (base64 in JSON) and the public hostname. The
//! request is plain HTTPS on port 443.
//!
//! References in the `cloudflared` source:
//!
//! * `cmd/cloudflared/tunnel/quick_tunnel.go` - `RunQuickTunnel` sends the
//!   POST; `QuickTunnelResponse` is the shape of the response.
//! * `cmd/cloudflared/tunnel/cmd.go` - the default of `quick-service` is
//!   `https://api.trycloudflare.com`.
//! * `quicktunnelauth` - with an email allow-list, the client sends
//!   `{"auth_mode":"otp"}` in place of an empty body. The allow-list stays on
//!   the client. Cloudflare only gets the information that OTP is necessary.
//!
//! # What this module does NOT do
//!
//! It does not open the tunnel edge connection. See [`crate::tunnel`].

mod error;
pub use error::*;

use std::sync::Arc;

use pod_core::{Class, Dial, Event, Failure, Stage};
use serde::{Deserialize, Serialize};

use crate::Host;

mod retry;
pub use retry::parse_retry_after;

pub use crate::services::default_quick_service;

/// The anonymous provisioning response, with only the fields that the crate
/// uses. The `QuickTunnel` of cloudflared also has `name`; the crate does not
/// use it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct QuickTunnel {
    /// The tunnel UUID.
    pub id: String,
    /// The public hostname, for example `foo-bar.trycloudflare.com`.
    pub hostname: String,
    /// The hex tag of the account of the tunnel.
    pub account_tag: String,
    /// The 32-byte tunnel secret in base64. cloudflared sends it to the edge
    /// as it is (`connection.Credentials.TunnelSecret`), with no hash.
    pub secret: String,
}

impl QuickTunnel {
    /// The public URL of this tunnel.
    pub fn url(&self) -> String {
        format!("https://{}", self.hostname)
    }

    /// The raw secret bytes, decoded from base64.
    ///
    /// cloudflared puts this value on the wire as `TunnelAuth.tunnelSecret`,
    /// with no hash. Thus a new implementation must not change it.
    pub fn secret_bytes(&self) -> Result<Vec<u8>, Failure> {
        decode_base64(&self.secret).ok_or_else(|| {
            let text = format!("tunnel secret is not valid base64 ({} bytes of text)", self.secret.len());
            Failure::new(Stage::EdgeRegister, Class::Protocol, text)
        })
    }
}

/// The top-level envelope: `{"success":bool,"result":{...},"errors":[...]}`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuickTunnelResponse {
    pub success: bool,
    #[serde(default)]
    pub result: Option<QuickTunnel>,
    #[serde(default)]
    pub errors: Vec<QuickTunnelError>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct QuickTunnelError {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
}

/// The authentication of the visitors of the tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Each person with the URL can get to the origin. The request body is
    /// empty.
    Public,
    /// A visitor must give a one-time PIN before it gets to the origin. The
    /// request body is `{"auth_mode":"otp"}`.
    ///
    /// The *client* applies the allow-list, as `cloudflared --allowed-mail`
    /// does. Cloudflare only gets the information that OTP is necessary. It
    /// never gets the addresses that are permitted.
    Otp,
}

impl AuthMode {
    /// The request body of the POST. `None` is an empty body.
    pub fn request_body(self) -> Option<&'static str> {
        match self {
            AuthMode::Public => None,
            AuthMode::Otp => Some(r#"{"auth_mode":"otp"}"#),
        }
    }
}

/// Provision a quick tunnel: `POST <endpoint>/tunnel`, with the client of
/// `host`.
///
/// `via` is a way out for this request only. With `None`, the request
/// takes the way out of `host`.
pub async fn request_quick_tunnel(
    host: &Host,
    endpoint: &str,
    auth_mode: AuthMode,
    user_agent: &str,
    timeout: std::time::Duration,
    via: Option<Arc<dyn Dial>>,
) -> Result<QuickTunnel, Failure> {
    request_once(host, endpoint, auth_mode, user_agent, timeout, via).await.map_err(Failure::from)
}

/// One provision request; [`ProvisionError`] says what a retry can do.
async fn request_once(
    host: &Host,
    endpoint: &str,
    auth_mode: AuthMode,
    user_agent: &str,
    timeout: std::time::Duration,
    via: Option<Arc<dyn Dial>>,
) -> Result<QuickTunnel, ProvisionError> {
    let url = format!("{}/tunnel", endpoint.trim_end_matches('/'));
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ProvisionError::BadEndpoint(url));
    }
    let mut request = pod_proxy::client::Request::new("POST", url)
        .header("Content-Type", "application/json")
        .header("User-Agent", user_agent)
        .timeout(timeout)
        .via(via);
    request.body = auth_mode.request_body().map(Into::into);
    let response = host.send(request).await;
    let response = response.map_err(ProvisionError::Http)?;
    let status = response.status;
    let text = response.text();
    let retry_after = response.header("retry-after").and_then(parse_retry_after);
    match parse_quick_tunnel_response(status, &text) {
        Err(ProvisionError::Status { code, body, .. }) => Err(ProvisionError::Status { code, body, retry_after }),
        other => other,
    }
}

/// The longest time that [`provision`] tries.
pub const PROVISION_BUDGET: std::time::Duration = std::time::Duration::from_secs(300);
/// The most attempts of [`provision`].
pub const PROVISION_ATTEMPTS: u32 = 5;

/// [`request_quick_tunnel`] with retries: a 429, a 5xx and a transport
/// error are tried again, after `Retry-After` or else a backoff with jitter
/// (2, 4, 8, 16 s), up to [`PROVISION_ATTEMPTS`] attempts in `budget`.
/// Another 4xx fails at once. A `Retry-After` that is longer than the time
/// left fails with the wait in the error.
pub async fn provision(
    host: &Host,
    endpoint: &str,
    auth_mode: AuthMode,
    user_agent: &str,
    timeout: std::time::Duration,
    budget: std::time::Duration,
) -> Result<QuickTunnel, Failure> {
    let start = std::time::Instant::now();
    let policy = crate::config::ReconnectPolicy {
        initial: std::time::Duration::from_secs(2),
        max: std::time::Duration::from_secs(16),
        ..Default::default()
    };
    let mut attempt = 0;
    let mut dns = retry::Dns::default();
    loop {
        attempt += 1;
        let err = match request_once(host, endpoint, auth_mode, user_agent, timeout, None).await {
            Ok(tunnel) => return Ok(tunnel),
            Err(err) => err,
        };
        let wait = match &err {
            ProvisionError::Status { code, retry_after, .. } if *code == 429 || (500..600).contains(code) => {
                retry_after.unwrap_or_else(|| policy.backoff_with_jitter(attempt))
            }
            // A refusal of the proxy and a TLS failure do not change on a
            // retry.
            ProvisionError::Http(inner)
                if matches!(crate::fail::stage_of(inner), Some(Stage::Proxy | Stage::Tls | Stage::Network)) =>
            {
                return Err(err.into());
            }
            // A name that does not resolve: one retry, then only when the
            // answering step changed.
            ProvisionError::Http(inner) if retry::is_dns(inner) => {
                if !dns.again(host, endpoint).await {
                    return Err(err.into());
                }
                policy.backoff_with_jitter(attempt)
            }
            ProvisionError::Http(_) => policy.backoff_with_jitter(attempt),
            _ => return Err(err.into()),
        };
        let left = budget.saturating_sub(start.elapsed());
        if attempt >= PROVISION_ATTEMPTS || wait > left {
            if wait > left {
                let text = format!(
                    "the quick-tunnel service asks to wait {} s, more than the {} s left",
                    wait.as_secs(),
                    left.as_secs()
                );
                host.event(Event::warn("quick.provision", text));
            }
            return Err(err.into());
        }
        let why = pod_core::failure::chain_of(&err);
        let text = format!("provisioning failed: {why}; the try {} is in {} s", attempt + 1, wait.as_secs());
        host.event(Event::warn("quick.provision", text));
        tokio::time::sleep(wait).await;
    }
}

/// The public host of a quick-tunnel URL, checked against the allowlist
/// `*.trycloudflare.com`. Thus a hostile API response cannot send the tool to
/// a different host.
pub fn validate_quick_hostname(raw: &str) -> Result<String, Failure> {
    let refuse = |text: String| Failure::new(Stage::EdgeRegister, Class::Protocol, text);
    let host = raw
        .trim()
        .trim_end_matches('/')
        .strip_prefix("https://")
        .unwrap_or(raw.trim().trim_end_matches('/'))
        .to_ascii_lowercase();
    let bad = host.is_empty() || host.contains(['/', '\\', '@', '?', '#', ':', '[', ']', ' ']);
    if bad {
        return Err(refuse(format!("quick tunnel returned a malformed hostname: {raw:?}")));
    }
    let zone = crate::services::text(crate::services::QUICK, "tunnel-zone");
    let Some(sub) = host.strip_suffix(&format!(".{zone}")) else {
        return Err(refuse(format!("quick tunnel hostname is not under {zone}: {host}")));
    };
    if sub.is_empty()
        || sub.contains('.')
        || sub.starts_with('-')
        || sub.ends_with('-')
        || sub.len() > 63
        || !sub.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(refuse(format!("quick tunnel hostname label is invalid: {host}")));
    }
    Ok(format!("https://{host}"))
}

/// Parse and check the body of a provisioning response.
///
/// This function is separate from [`request_quick_tunnel`], so that a test
/// can use recorded real responses with no live service.
pub(crate) fn parse_quick_tunnel_response(status: u16, body: &str) -> Result<QuickTunnel, ProvisionError> {
    if !(200..300).contains(&status) {
        // The body of an error, for one line of a terminal: 1 KiB at most,
        // with no control character and no escape sequence.
        let mut clipped = pod_core::text::one_line(body);
        if clipped.len() > 1024 {
            let mut cut = 1024;
            while !clipped.is_char_boundary(cut) {
                cut -= 1;
            }
            clipped.truncate(cut);
        }
        return Err(ProvisionError::Status { code: status, body: clipped, retry_after: None });
    }

    // The provisioning body has the `secret`, the `account_tag` and the `id`
    // of the tunnel: the credentials that the edge expects at registration.
    // Thus the body of a decode failure is mostly credentials. This error
    // goes to the stderr of the operator and to each CI log. Redact the body
    // before the error includes a part of it.
    let parsed: QuickTunnelResponse =
        serde_json::from_str(body).map_err(|e| ProvisionError::Decode(format!("{e}; body was: {}", redact(body))))?;

    if !parsed.success {
        return Err(ProvisionError::Service(parsed.errors));
    }

    parsed.result.ok_or(ProvisionError::MissingResult)
}

/// Replace the fields with credentials of a provisioning body with a marker.
/// Then clip the body to a length that a person can read.
///
/// A provisioning response is mostly secret. `secret` is a 32-byte base64
/// blob, and `account_tag` identifies the account. The edge expects both at
/// registration. The body in an error helps to find the cause of a bad
/// response. But the credentials in a log are a leak that its readers cannot
/// see. Thus the values are replaced, and the body stays. The diagnostic
/// stays, and nothing important is lost.
fn redact(body: &str) -> String {
    pod_core::text::body_line(body, &["secret", "account_tag", "tunnel_secret", "id"], 200)
}

/// A minimal decoder of standard base64 for the tunnel secret.
///
/// The crate has its own decoder, not a dependency: the secret is standard
/// base64 with padding, and the list of dependencies stays small.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;

    for &b in &bytes {
        if b == b'=' {
            break;
        }
        let v = value(b)?;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

#[cfg(test)]
mod tests;
