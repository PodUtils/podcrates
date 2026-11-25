use super::redact;

/// A body shaped like the real provisioning response, with values that are
/// recognisable so a leak in a log is visible.
const REAL_BODY: &str = r#"{"success":true,"result":{"id":"2983af16-7c68-4ef0-9590-e05f35a0cd7b","name":"qt-x","hostname":"foo.trycloudflare.com","account_tag":"5ab4e9dfbd435d24068829fda0077963","secret":"GlUwcZAOuFFaHRtTTFqbk1WVmpWMHFzOGdSSUthbG1K"}}"#;

#[test]
fn redaction_removes_every_credential_from_the_error_text() {
    let redacted = redact(REAL_BODY);
    assert!(
        !redacted.contains("GlUwcZAOuFFaHRtTTFqbk1WVmpWMHFzOGdSSUthbG1K"),
        "the tunnel secret must not survive redaction: {redacted}"
    );
    assert!(
        !redacted.contains("5ab4e9dfbd435d24068829fda0077963"),
        "the account tag must not survive redaction: {redacted}"
    );
    assert!(
        !redacted.contains("2983af16-7c68-4ef0-9590-e05f35a0cd7b"),
        "the tunnel id must not survive redaction: {redacted}"
    );
    assert!(redacted.contains("<redacted>"), "the redaction marker should be visible: {redacted}");
}

#[test]
fn redaction_keeps_the_fields_a_human_needs() {
    let redacted = redact(REAL_BODY);
    assert!(
        redacted.contains("foo.trycloudflare.com"),
        "the hostname is the part that identifies which tunnel this was: {redacted}"
    );
    assert!(redacted.contains(r#""secret""#), "the key should remain: {redacted}");
}

#[test]
fn redaction_survives_a_body_that_is_not_json() {
    // A hostile or intercepted endpoint can return anything. The error path
    // must not panic on it.
    for body in [
        "",
        "{",
        "not json at all",
        "{\"secret\":",
        "{\"secret\":\"unterminated",
        r#"{"secret":"a","secret":"b"}"#,
        r#"{"nested":{"account_tag":"x"},"id":"y"}"#,
    ] {
        let out = redact(body);
        assert!(!out.contains("unterminated") || body.len() < 40, "{body:?} -> {out:?}");
    }
}

/// The body of a decode failure is one line that is safe for a terminal.
#[test]
fn the_body_of_a_decode_failure_is_one_safe_line() {
    let out = redact("not\x1b[2J json\nat all");
    assert_eq!(out, "not json at all");
    assert!(redact(&"é".repeat(400)).len() <= 200);
}

use super::*;
use pod_core::{Class, Failure, Stage};

const OK_BODY: &str = r#"{"success":true,"result":{"id":"6ff42ae2-765d-4adf-8112-31c55c1551ef","hostname":"t.trycloudflare.com","account_tag":"0123456789abcdef","secret":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="}}"#;

/// A local endpoint that gives `answers` in order, one per connection.
/// Gives its URL and the count of requests.
fn scripted(
    answers: Vec<(u16, &'static str, &'static str)>,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = hits.clone();
    std::thread::spawn(move || {
        let mut answers = answers.into_iter();
        for mut s in listener.incoming().flatten() {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let Some((code, extra, body)) = answers.next() else {
                break;
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (url, hits)
}

fn hits(h: &std::sync::atomic::AtomicUsize) -> usize {
    h.load(std::sync::atomic::Ordering::SeqCst)
}

const BUDGET: std::time::Duration = std::time::Duration::from_secs(60);
const TO: std::time::Duration = std::time::Duration::from_secs(5);

/// The refusal of a provision as fields.
#[test]
fn the_refusal_in_an_error_chain() {
    let err = crate::fail::wrap(
        Failure::from(ProvisionError::Status {
            code: 429,
            body: "error code: 1015".into(),
            retry_after: Some(std::time::Duration::from_secs(1379)),
        }),
        PROVISIONING,
    );
    let r = retry_of(&err).unwrap();
    assert_eq!(r, Refusal { status: 429, code: Some(1015), retry_after_s: Some(1379), retryable: true });
    let other = Failure::from(ProvisionError::Status { code: 404, body: String::new(), retry_after: None });
    assert!(!retry_of(&other).unwrap().retryable);
    assert!(retry_of(&Failure::new(Stage::Host, Class::Config, "other")).is_none());
}

/// The failure of a provision is its step, and the error is the cause: a
/// reader that joins the chain says the words of the error once; the
/// context of the plan with the same step adds nothing.
#[test]
fn a_joined_provision_failure_says_each_cause_once() {
    let status = || ProvisionError::Status { code: 503, body: "busy".into(), retry_after: None };
    let inner = || Failure::new(Stage::Proxy, Class::Refused, "POST https://a/tunnel: 403 not on the allowlist");
    for failure in [Failure::from(status()), Failure::from(ProvisionError::Http(inner()))] {
        let line = crate::testing::each_text(&failure);
        assert_eq!(line, failure.chain());
        assert_eq!(line.matches(PROVISIONING).count(), 1, "{line}");
        assert_eq!(failure.text(), PROVISIONING);
    }
    let line = crate::testing::each_text(&Failure::from(status()));
    assert_eq!(line, "provisioning quick tunnel: quick-tunnel service returned HTTP 503: busy");
    let line = crate::testing::each_text(&Failure::from(ProvisionError::Http(inner())));
    assert_eq!(line.matches("403 not on the allowlist").count(), 1, "{line}");
    let planned = crate::fail::wrap(Failure::from(status()), PROVISIONING);
    assert_eq!(crate::testing::each_text(&planned), crate::testing::each_text(&Failure::from(status())));
}

#[tokio::test]
async fn a_429_waits_for_retry_after() {
    pod_testkit::need_loopback!();
    let (url, count) = scripted(vec![(429, "Retry-After: 1\r\n", "slow down"), (200, "", OK_BODY)]);
    let start = std::time::Instant::now();
    let tunnel = provision(&crate::testing::host(), &url, AuthMode::Public, "t", TO, BUDGET).await.unwrap();
    assert_eq!(tunnel.hostname, "t.trycloudflare.com");
    assert!(start.elapsed() >= std::time::Duration::from_secs(1));
    assert_eq!(hits(&count), 2);
}

#[tokio::test]
async fn a_long_retry_after_fails_with_the_wait() {
    pod_testkit::need_loopback!();
    let (url, count) = scripted(vec![(429, "Retry-After: 9999\r\n", "error code: 1015")]);
    let err = provision(&crate::testing::host(), &url, AuthMode::Public, "t", TO, BUDGET).await.unwrap_err();
    let text = err.chain();
    assert!(text.contains("HTTP 429 (Retry-After: 9999 s): error code: 1015"), "{text}");
    assert_eq!(hits(&count), 1);
}

#[tokio::test]
async fn a_5xx_is_retried_and_a_403_is_not() {
    pod_testkit::need_loopback!();
    let (url, count) = scripted(vec![
        (500, "Retry-After: 0\r\n", ""),
        (502, "Retry-After: 0\r\n", ""),
        (503, "Retry-After: 0\r\n", ""),
        (200, "", OK_BODY),
    ]);
    provision(&crate::testing::host(), &url, AuthMode::Public, "t", TO, BUDGET).await.unwrap();
    assert_eq!(hits(&count), 4);

    let (url, count) = scripted(vec![(403, "", "forbidden\u{1b}[2J\u{7}\rnow")]);
    let err = provision(&crate::testing::host(), &url, AuthMode::Public, "t", TO, BUDGET).await.unwrap_err();
    assert_eq!(hits(&count), 1);
    let text = err.chain();
    assert!(text.ends_with("HTTP 403: forbidden now"), "{text:?}");
}

#[test]
fn retry_after_values() {
    let secs = std::time::Duration::from_secs;
    assert_eq!(parse_retry_after("120"), Some(secs(120)));
    assert_eq!(parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(std::time::Duration::ZERO));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let later = now.as_secs() as i64 + 3600;
    let wait = parse_retry_after(&pod_core::clock::http_date(later)).unwrap();
    assert!(wait.as_secs() > 3590 && wait.as_secs() <= 3600, "{wait:?}");
    assert_eq!(parse_retry_after("soon"), None);
}

// A date out of the range of an IMF-fixdate is no wait, and a long year
// does not overflow the count of seconds.
#[test]
fn a_retry_after_date_out_of_range_is_refused() {
    for bad in
        ["Sun, 32 Nov 1994 08:49:37 GMT", "Sun, 06 Nov 1994 25:61:00 GMT", "Sun, 06 Nov 99999999999999 08:49:37 GMT"]
    {
        assert_eq!(parse_retry_after(bad), None, "{bad}");
    }
}

/// A response of a real `POST https://api.trycloudflare.com/tunnel` of the
/// live service. The tunnel does not exist now; these tests read only the
/// shape.
const REAL_RESPONSE: &str = r#"{
  "id": "c1267064-5604-4d16-9a83-66b7ed37f182",
  "name": "operator-icon-sheffield-compression",
  "hostname": "operator-icon-sheffield-compression.trycloudflare.com",
  "account_tag": "5ab4e9dfbd435d24068829fda0077963",
  "secret": "GlUwcZAOuFFaHRtTTFqbk1WVmpWMHFzOGdSSUthbG1K",
  "success": true,
  "result": {
    "id": "c1267064-5604-4d16-9a83-66b7ed37f182",
    "name": "operator-icon-sheffield-compression",
    "hostname": "operator-icon-sheffield-compression.trycloudflare.com",
    "account_tag": "5ab4e9dfbd435d24068829fda0077963",
    "secret": "GlUwcZAOuFFaHRtTTFqbk1WVmpWMHFzOGdSSUthbG1K"
  },
  "errors": [],
  "messages": []
}"#;

#[test]
fn parses_a_real_provisioning_response() {
    let tunnel = parse_quick_tunnel_response(200, REAL_RESPONSE).expect("should parse");
    assert_eq!(tunnel.hostname, "operator-icon-sheffield-compression.trycloudflare.com");
    assert_eq!(tunnel.url(), "https://operator-icon-sheffield-compression.trycloudflare.com");
    assert_eq!(tunnel.id, "c1267064-5604-4d16-9a83-66b7ed37f182");
    assert_eq!(tunnel.account_tag.len(), 32, "account tag is a 32-char hex string");
}

#[test]
fn secret_decodes_to_the_documented_32_bytes() {
    let tunnel = parse_quick_tunnel_response(200, REAL_RESPONSE).expect("should parse");
    let bytes = tunnel.secret_bytes().expect("secret should be base64");
    // cloudflared uses this as TunnelSecret and puts it raw on the wire.
    assert_eq!(bytes.len(), 32, "tunnel secret must be 32 raw bytes");
}

#[test]
fn hostname_is_a_trycloudflare_subdomain() {
    let tunnel = parse_quick_tunnel_response(200, REAL_RESPONSE).expect("should parse");
    assert!(
        tunnel.hostname.ends_with(".trycloudflare.com"),
        "quick tunnel hostnames are under trycloudflare.com: {}",
        tunnel.hostname
    );
    let label = tunnel.hostname.strip_suffix(".trycloudflare.com").expect("suffix checked above");
    assert!(!label.is_empty(), "there must be a leading label");
    assert!(
        label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "the label must be ascii alphanumerics and hyphens: {label}"
    );
    assert!(!tunnel.hostname.contains('/'), "hostname must not carry a scheme or path");
}

#[test]
fn rejects_a_success_false_envelope() {
    let body = r#"{"success":false,"result":null,"errors":[{"code":1001,"message":"nope"}]}"#;
    match parse_quick_tunnel_response(200, body) {
        Err(ProvisionError::Service(errs)) => {
            assert_eq!(errs.len(), 1);
            assert_eq!(errs[0].code, 1001);
            assert_eq!(errs[0].message, "nope");
        }
        other => panic!("expected Service error, got {other:?}"),
    }
}

#[test]
fn rejects_success_with_no_result() {
    let body = r#"{"success":true,"errors":[]}"#;
    match parse_quick_tunnel_response(200, body) {
        Err(ProvisionError::MissingResult) => {}
        other => panic!("expected MissingResult, got {other:?}"),
    }
}

#[test]
fn rejects_a_non_2xx_status() {
    match parse_quick_tunnel_response(403, "forbidden") {
        Err(ProvisionError::Status { code, .. }) => assert_eq!(code, 403),
        other => panic!("expected Status error, got {other:?}"),
    }
}

#[test]
fn rejects_malformed_json() {
    match parse_quick_tunnel_response(200, "not json at all") {
        Err(ProvisionError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

#[test]
fn public_mode_sends_an_empty_body_and_otp_sends_the_auth_mode() {
    assert_eq!(AuthMode::Public.request_body(), None);
    assert_eq!(AuthMode::Otp.request_body(), Some(r#"{"auth_mode":"otp"}"#));
}

const T: std::time::Duration = std::time::Duration::from_secs(5);

#[tokio::test]
async fn endpoint_must_be_an_absolute_http_url() {
    let host = crate::testing::host();
    let err = request_quick_tunnel(&host, "ftp://example.com", AuthMode::Public, "t", T, None).await.unwrap_err();
    let err = crate::fail::find::<ProvisionError>(&err).expect("a provision error");
    assert!(matches!(err, ProvisionError::BadEndpoint(_)), "got {err:?}");
}

#[tokio::test]
async fn trailing_slash_on_the_endpoint_is_not_doubled() {
    let host = crate::testing::host();
    let err = request_quick_tunnel(&host, "not-a-url/", AuthMode::Public, "t", T, None).await.unwrap_err();
    match crate::fail::find::<ProvisionError>(&err).expect("a provision error") {
        ProvisionError::BadEndpoint(url) => {
            assert!(url.ends_with("/tunnel"), "unexpected url {url}");
            assert!(!url.contains("//tunnel"), "double slash in {url}");
        }
        other => panic!("expected BadEndpoint, got {other:?}"),
    }
}

#[tokio::test]
async fn a_closed_proxy_is_a_transport_failure() {
    let proxy = crate::testing::proxied("http://127.0.0.1:1");
    let err = request_quick_tunnel(
        &crate::testing::host(),
        "https://api.trycloudflare.com",
        AuthMode::Public,
        "t",
        T,
        Some(proxy),
    )
    .await
    .unwrap_err();
    let err = crate::fail::find::<ProvisionError>(&err).expect("a provision error");
    assert!(
        matches!(err, ProvisionError::Http(_) | ProvisionError::Status { .. }),
        "expected a transport failure, got {err:?}"
    );
}

#[test]
fn quick_hosts() {
    assert_eq!(
        validate_quick_hostname("https://abc-123.trycloudflare.com/").unwrap(),
        "https://abc-123.trycloudflare.com"
    );
    assert!(validate_quick_hostname("https://evil.com").is_err());
    assert!(validate_quick_hostname("https://a.b.trycloudflare.com").is_err());
    assert!(validate_quick_hostname("http://abc.trycloudflare.com").is_err());
}
