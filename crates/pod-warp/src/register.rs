//! Anonymous WARP registration.
//!
//! Two calls, the first with no credential:
//!
//! 1. `POST /{version}/reg`: create a device. The body carries a throwaway
//!    X25519 public key so that the service accepts it; the private half is
//!    dropped, as MASQUE does not use it.
//! 2. `PATCH /{version}/reg/{id}`: enrol a P-256 public key (X.509 SPKI,
//!    base64) and switch the device from `wireguard` to `masque`. Then the
//!    record's `config.peers[0].endpoint` names the MASQUE front door, and
//!    the TLS client certificate authenticates the tunnel.

use std::time::Duration;

use pod_core::entropy::{Entropy as _, OsEntropy};
use pod_core::{Class, Failure, Stage};
use pod_proxy::client::Request;
use serde::Serialize;

use crate::api::{Api, ApiError, success};
use crate::device::{Device, redact_account};
use crate::hints::DeviceHints;

/// The `POST /reg` body. Each value is a string, as the Android client
/// sends it.
#[derive(Debug, Serialize)]
struct RegistrationRequest {
    key: String,
    install_id: String,
    fcm_token: String,
    tos: String,
    model: String,
    serial_number: String,
    os_version: String,
    key_type: String,
    tunnel_type: String,
    locale: String,
}

/// The `PATCH /reg/{id}` body that enrols a MASQUE key.
#[derive(Debug, Serialize)]
struct EnrolRequest {
    key: String,
    key_type: String,
    tunnel_type: String,
    /// The name of the device in the dashboard (PodUtils/podnet issue 141).
    /// Absent by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// The time of now as RFC 3339 with milliseconds, in UTC.
pub(crate) fn now_rfc3339_millis() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let (y, mo, d, h, mi, s) = civil_from_unix(now.as_secs());
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{:03}Z", now.subsec_millis())
}

/// The civil UTC date and time of Unix seconds.
fn civil_from_unix(secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let (y, m, d) = pod_core::clock::civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

fn random_bytes<const N: usize>() -> Result<[u8; N], Failure> {
    let mut out = [0u8; N];
    OsEntropy
        .fill(&mut out)
        .map_err(|_| Failure::new(Stage::Warp, Class::Unavailable, "this host's random source gave no bytes"))?;
    Ok(out)
}

fn b64(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn with_json<T: Serialize>(request: Request, body: &T) -> Request {
    let text = serde_json::to_string(body).unwrap_or_default();
    request.json(text)
}

/// The device of an answer. A body that does not decode is not shown: the
/// answer of `POST /reg` holds the token.
fn device_of(response: pod_proxy::http::Response, what: &str) -> Result<Device, Failure> {
    let response = success(response, what)?;
    serde_json::from_slice(&response.body).map_err(|_| {
        let text = format!("{what}: the WARP API answer does not decode ({} bytes)", response.body.len());
        Failure::new(Stage::Warp, Class::Protocol, text)
    })
}

fn json_of(response: pod_proxy::http::Response, what: &str) -> Result<serde_json::Value, Failure> {
    let response = success(response, what)?;
    serde_json::from_slice(&response.body).map_err(|_| {
        let text = format!("{what}: the WARP API answer does not decode ({} bytes)", response.body.len());
        Failure::new(Stage::Warp, Class::Protocol, text)
    })
}

/// Create an anonymous device with a throwaway X25519 public key: the API
/// checks its shape, not that anybody holds the private half.
pub async fn create_device(api: &Api, hints: &DeviceHints) -> Result<Device, Failure> {
    create_device_with_key(api, &b64(&random_bytes::<32>()?), hints).await
}

/// Register a WireGuard device with the X25519 public key `key` (base64);
/// podnet's `warp export` keeps the private half.
pub async fn create_device_with_key(api: &Api, key: &str, hints: &DeviceHints) -> Result<Device, Failure> {
    hints.check()?;
    let serial: String = random_bytes::<8>()?.iter().map(|b| format!("{b:02x}")).collect();
    let body = RegistrationRequest {
        key: key.to_string(),
        install_id: String::new(),
        fcm_token: String::new(),
        tos: now_rfc3339_millis(),
        model: hints.model.clone().unwrap_or_else(|| "PC".into()),
        serial_number: serial,
        os_version: String::new(),
        key_type: "curve25519".into(),
        tunnel_type: "wireguard".into(),
        locale: hints.locale.clone().unwrap_or_else(|| "en_US".into()),
    };
    let mut request = with_json(api.request("POST", "/reg", None), &body);
    if let Some(team) = api.team() {
        request = request.header("CF-Access-Jwt-Assertion", team);
    }
    let response = api.send(request).await?;
    if let Some(failure) = registration_error(response.status, response.header("retry-after"), &response.text()) {
        return Err(failure);
    }
    device_of(response, "POST /reg")
}

/// The wait of a `Retry-After`: seconds, or an HTTP date.
fn retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = pod_core::clock::parse_http_date(value)?;
    let now = crate::identity::unix_now();
    Some(Duration::from_secs(u64::try_from(at - now).unwrap_or(0)))
}

/// The failure of a refused registration (PodUtils/podnet issue 187): a
/// 429, or another error status with the text of error 1015, names the
/// remedy and the Retry-After. None for another status. A success (2xx) is
/// never this failure: its body is random ids, keys and the token of the
/// device, and can hold the digits 1015.
pub fn registration_error(status: u16, retry: Option<&str>, body: &str) -> Option<Failure> {
    let limited = status == 429 || (status >= 400 && body.contains("1015"));
    if !limited {
        return None;
    }
    let wait = retry
        .and_then(retry_after)
        .map(|d| format!("; the service asks to wait {} s", d.as_secs()))
        .unwrap_or_default();
    let cause = ApiError { status, body: body.chars().take(200).collect() };
    let text = format!(
        "the WARP service limits new devices for each address (HTTP {status}, error 1015){wait}; reuse a saved identity in place of a new device, or wait"
    );
    Some(Failure::caused(Stage::Warp, Class::TempFail, text, cause))
}

/// Enrol a MASQUE identity: send the P-256 public key (X.509 SPKI, base64)
/// and switch the device to the tunnel type `masque`. `name` names the
/// device in the dashboard.
pub async fn enrol_masque(
    api: &Api,
    device_id: &str,
    token: &str,
    spki_b64: &str,
    name: Option<String>,
) -> Result<Device, Failure> {
    let body = EnrolRequest { key: spki_b64.into(), key_type: "secp256r1".into(), tunnel_type: "masque".into(), name };
    let request = with_json(api.request("PATCH", &format!("/reg/{device_id}"), Some(token)), &body);
    device_of(api.send(request).await?, "PATCH /reg/{id}")
}

/// Read the device record again (`GET /reg/{id}`).
pub async fn get_device(api: &Api, device_id: &str, token: &str) -> Result<Device, Failure> {
    let request = api.request("GET", &format!("/reg/{device_id}"), Some(token));
    device_of(api.send(request).await?, "GET /reg/{id}")
}

/// Read the account of the device (`GET /reg/{id}/account`): the account
/// type, the WARP+ state and the data quota. The license key is hidden.
pub async fn get_account(api: &Api, device_id: &str, token: &str) -> Result<serde_json::Value, Failure> {
    let request = api.request("GET", &format!("/reg/{device_id}/account"), Some(token));
    let mut account = json_of(api.send(request).await?, "GET /reg/{id}/account")?;
    redact_account(&mut account);
    Ok(account)
}

/// Attach a WARP+ license key to the device (`PUT /reg/{id}/account`). The
/// answer hides the key. A refused key has the class of
/// [`crate::identity::license_class`].
pub async fn set_license(api: &Api, device_id: &str, token: &str, license: &str) -> Result<serde_json::Value, Failure> {
    let body = serde_json::json!({ "license": license }).to_string();
    let request = api.request("PUT", &format!("/reg/{device_id}/account"), Some(token)).json(body);
    let mut account = json_of(api.send(request).await?, "PUT /reg/{id}/account").map_err(|f| {
        let class = crate::identity::license_class(crate::api::api_status(&f));
        Failure::caused(Stage::Warp, class, "the WARP license", f)
    })?;
    redact_account(&mut account);
    Ok(account)
}

/// Remove the device from the WARP service (`DELETE /reg/{id}`).
pub async fn delete_device(api: &Api, device_id: &str, token: &str) -> Result<(), Failure> {
    let request = api.request("DELETE", &format!("/reg/{device_id}"), Some(token));
    success(api.send(request).await?, "DELETE /reg/{id}").map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    // PodUtils/podnet issue 187: a 429 of the registration names the remedy.
    #[test]
    fn a_429_on_registration_names_the_remedy() {
        let err = registration_error(429, Some("120"), "error code: 1015").unwrap();
        let text = err.to_string();
        assert!(text.contains("reuse a saved identity"), "{text}");
        assert!(text.contains("wait 120 s"), "{text}");
        assert_eq!(crate::api::api_status(&err), Some(429));
        assert_eq!(err.class.exit_code(), 75);
        assert!(registration_error(400, None, "bad key").is_none());
        assert!(registration_error(403, None, "error code: 1015").is_some());
    }

    // A 200 whose body holds the digits 1015 is a registered device, not the
    // limit of error 1015.
    #[test]
    fn a_success_with_1015_in_the_body_is_not_the_limit() {
        let body = r#"{"id":"a1015b","key":"x","token":"t-1015","account":{"id":"f"}}"#;
        assert!(registration_error(200, None, body).is_none());
        assert!(registration_error(201, None, body).is_none());
    }

    // PodUtils/podnet issue 141: the hints are checked; the default body is
    // as before.
    #[test]
    fn the_device_hints() {
        assert!(DeviceHints::default().check().is_ok());
        let ok =
            DeviceHints { name: Some("ci-runner-7".into()), model: Some("Linux".into()), locale: Some("de_DE".into()) };
        assert!(ok.check().is_ok());
        for bad in [
            DeviceHints { name: Some(String::new()), ..Default::default() },
            DeviceHints { name: Some("x".repeat(65)), ..Default::default() },
            DeviceHints { name: Some("a\nb".into()), ..Default::default() },
            DeviceHints { model: Some("m".repeat(33)), ..Default::default() },
            DeviceHints { locale: Some("english".into()), ..Default::default() },
            DeviceHints { locale: Some("EN_us".into()), ..Default::default() },
        ] {
            let err = bad.check().unwrap_err();
            assert_eq!(err.class, Class::Usage, "{bad:?}");
        }
        let body = serde_json::to_value(EnrolRequest {
            key: "k".into(),
            key_type: "secp256r1".into(),
            tunnel_type: "masque".into(),
            name: None,
        })
        .unwrap();
        assert!(body.get("name").is_none(), "{body}");
    }

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        // 2026-10-31T00:00:00Z
        assert_eq!(civil_from_unix(1_793_404_800), (2026, 10, 31, 0, 0, 0));
    }

    #[test]
    fn timestamp_shape() {
        let ts = now_rfc3339_millis();
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
    }

    #[test]
    fn a_retry_after_is_seconds_or_a_date() {
        assert_eq!(retry_after(" 30 "), Some(Duration::from_secs(30)));
        assert_eq!(retry_after("Thu, 01 Jan 1970 00:00:00 GMT"), Some(Duration::ZERO));
        assert_eq!(retry_after("soon"), None);
    }
}
