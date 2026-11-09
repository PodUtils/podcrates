use super::wire::*;
use super::*;
use crate::net::Transport;

/// One HTTP/1.0 reply on loopback, then the body: the origin of the test,
/// with no registry near it.
fn serve_once(body: Vec<u8>) -> u16 {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback binds where tests run");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut req = [0u8; 4096];
        let _ = s.read(&mut req);
        let head = format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        s.write_all(head.as_bytes()).unwrap();
        s.write_all(&body).unwrap();
    });
    port
}

const TEST_LIMITS: Limits = Limits { head: Duration::from_secs(5), idle: Duration::from_secs(5) };

/// The HTTP client of `pod-proxy` with no proxy, whatever the environment
/// of the test says, and the compiled-in roots.
pub(crate) fn http_client() -> (pod_proxy::blocking::Client, Trust) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-oci"))));
    let settings = pod_proxy::Settings::new("pod-oci", pod_proxy::ProxyChoice::Direct);
    let dialer = pod_proxy::blocking::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()))
        .expect("the runtime of the dialer starts");
    let client = pod_proxy::blocking::Client::new(context, &dialer).with_user_agent("pod-oci-test");
    (client, Trust::Default)
}

/// A transport with no proxy.
fn net() -> Net {
    let (client, verified) = http_client();
    Net { client, verified, unverified: None }
}

fn client() -> Client {
    let (http, verified) = http_client();
    Client::new(http, verified)
}

fn get(port: u16) -> Body {
    match net().send("GET", &format!("http://127.0.0.1:{port}/blob"), &[], true, TEST_LIMITS) {
        Ok(body) => body,
        Err(e) => panic!("the loopback origin answers: {e}"),
    }
}

/// A body past its declared size gets a refusal in the stream, and the
/// staged file never passes the cap: the defect was a read to the end that
/// named the lie only after the bytes were on disk.
#[test]
fn a_body_past_its_declared_size_is_refused_mid_stream() {
    pod_testkit::need_loopback!();
    let port = serve_once(vec![7u8; 1 << 20]);
    let resp = get(port);
    let mut sink = Vec::new();
    let want = Digest::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    let err = match Client::drain(resp, &mut sink, &want, Some(100), None, "/test") {
        Err(e @ Cut::Again(_)) => e,
        Err(Cut::Stop(e)) => panic!("a cut transfer is tried again, not stopped: {e}"),
        Ok(()) => panic!("a body past its size is refused"),
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("declares 100 byte(s)") && msg.contains("partial was discarded"),
        "the refusal names the declared size: {msg}"
    );
    assert!(sink.len() <= 101, "staged {} byte(s): reads clamp at the cap", sink.len());
}

/// An exact body lands: the cap refuses the extra byte, not the declared
/// ones.
#[test]
fn an_exact_body_lands_whole() {
    pod_testkit::need_loopback!();
    let body = vec![9u8; 100];
    let mut v = Verifier::new(Vec::new());
    v.write_all(&body).unwrap();
    let want = v.digest();
    let port = serve_once(body.clone());
    let resp = get(port);
    let mut sink = Vec::new();
    assert!(Client::drain(resp, &mut sink, &want, Some(100), None, "/test").is_ok());
    assert_eq!(sink, body);
}

/// The pages follow `rel="next"` and nothing else, and an empty or absent
/// target ends the walk.
#[test]
fn a_next_link_is_followed_and_a_last_link_is_not() {
    assert_eq!(
        next_link(r#"</v2/repo/tags/list?last=b&n=2>; rel="next""#),
        Some("/v2/repo/tags/list?last=b&n=2".to_string())
    );
    assert_eq!(
        next_link(r#"</v2/repo/tags/list?last=b&n=2>; rel="next", </v2/repo/tags/list?n=2>; rel="last""#),
        Some("/v2/repo/tags/list?last=b&n=2".to_string())
    );
    assert_eq!(next_link(r#"</v2/repo/tags/list?n=2>; rel="last""#), None);
    assert_eq!(next_link(r#"<>; rel="next""#), None);
}

#[test]
fn a_bearer_challenge_parses_into_realm_service_and_scope() {
    let got = parse_challenge(
        r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/alpine:pull""#,
    );
    assert_eq!(got["realm"], "https://auth.docker.io/token");
    assert_eq!(got["service"], "registry.docker.io");
    assert_eq!(got["scope"], "repository:library/alpine:pull");
}

#[test]
fn a_scope_carrying_a_comma_is_not_truncated_at_it() {
    let got = parse_challenge(r#"Bearer realm="https://x/token",scope="repository:a:pull,push",service="x""#);
    assert_eq!(got["scope"], "repository:a:pull,push");
    assert_eq!(got["service"], "x");
}

#[test]
fn a_url_query_never_survives_into_a_message() {
    assert_eq!(redact("https://cdn.example/blob?X-Amz-Signature=deadbeef"), "https://cdn.example/blob?<redacted>");
    assert_eq!(redact("https://x/v2/"), "https://x/v2/");
}

#[test]
fn the_backoff_honours_the_servers_delay_and_still_caps_it() {
    assert_eq!(backoff(1, Some(Duration::from_secs(5))), Duration::from_secs(5));
    assert_eq!(backoff(1, Some(Duration::from_secs(600))), BACKOFF_CAP);
    assert!(backoff(1, None) < backoff(3, None));
    assert!(backoff(20, None) <= BACKOFF_CAP);
}

#[test]
fn only_the_statuses_worth_repeating_are_retried() {
    assert!(retryable(429) && retryable(503));
    // A 404 or a 403 tried three times is three wrong answers and a slower
    // error message.
    assert!(!retryable(404) && !retryable(403) && !retryable(401));
}

#[test]
fn a_retry_after_that_is_not_a_number_falls_back_rather_than_waiting_forever() {
    assert_eq!(parse_retry_after("7"), Some(Duration::from_secs(7)));
    assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
}

#[test]
fn a_token_is_keyed_by_repository_and_not_by_host_alone() {
    // A cache with no repository in its key gives one token to another
    // repository.
    assert_ne!(
        key("registry-1.docker.io", "repository:library/alpine:pull"),
        key("registry-1.docker.io", "repository:library/busybox:pull")
    );
}

#[test]
fn a_registry_error_body_is_read_for_its_message() {
    let body = r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"manifest unknown"}]}"#;
    assert_eq!(registry_message(body).unwrap(), "MANIFEST_UNKNOWN: manifest unknown");
    assert!(registry_message("not json").is_none());
}

/// Split a request URL the way the error renderer needs it: any scheme,
/// and the path kept apart from the host.
#[test]
fn a_request_url_splits_into_host_and_path() {
    assert_eq!(
        split_host_path("https://registry.example/v2/a/manifests/3.20"),
        ("registry.example".to_string(), "/v2/a/manifests/3.20".to_string())
    );
    assert_eq!(split_host_path("http://localhost:5000/v2/"), ("localhost:5000".to_string(), "/v2/".to_string()));
    assert_eq!(split_host_path("registry.example/v2/"), ("registry.example".to_string(), "/v2/".to_string()));
}

/// Each cause that the transport names is plain lower case, by the stage
/// that broke: not `Dns Failed`.
#[test]
fn every_transport_cause_renders_in_plain_lowercase() {
    pod_testkit::need_loopback!();
    let t = refused_transport();
    let cause = t.why.split(':').next().unwrap_or("");
    assert_eq!(cause, "connection failed", "{}", t.why);
    assert_eq!(t.host, "127.0.0.1:1");
}

/// One refused loopback port, no network out of the machine: the source of
/// each transport test below.
fn refused_transport() -> Transport {
    match net().send("GET", "https://127.0.0.1:1/v2/", &[], true, TEST_LIMITS) {
        Err(Refusal::Transport(t)) => t,
        Err(e) => panic!("a refused port is a transport error, not {e}"),
        Ok(body) => panic!("a refused port is a transport error, not a status {}", body.status()),
    }
}

/// Not `GET url: transport: url: Dns Failed`, with the URL twice and a kind
/// in capitals: the host once, in plain lower case, with the chain after
/// the cause on the same line.
#[test]
fn a_transport_error_names_the_host_once_in_plain_words() {
    pod_testkit::need_loopback!();
    let t = refused_transport();
    let rendered = format!("{}", transport_error("https://127.0.0.1:1/v2/", t));
    assert!(rendered.contains("127.0.0.1:1: connection failed"), "host once with a plain lowercase cause: {rendered}");
    assert!(
        !rendered.contains("Connection Failed") && !rendered.contains("https://"),
        "no capitalised kind and no repeated URL: {rendered}"
    );
    assert_eq!(rendered.matches("127.0.0.1").count(), 1, "the host appears exactly once: {rendered}");
    assert!(!rendered.contains('\n'), "the chain stays on the same line: {rendered}");
}

/// After the last try, `get` refuses with the same shape of one host, not
/// with the `GET url` prefix twice.
#[test]
fn an_exhausted_transport_retry_names_the_host_once() {
    pod_testkit::need_loopback!();
    let mut client = client();
    let err = match client.manifest("127.0.0.1:1", "library/alpine", "3.20") {
        Ok(_) => panic!("a refused port pulls nothing"),
        Err(e) => e,
    };
    let rendered = format!("{err}");
    assert!(rendered.contains("127.0.0.1:1: connection failed"), "host once with a plain lowercase cause: {rendered}");
    assert_eq!(rendered.matches("127.0.0.1").count(), 1, "the host appears exactly once: {rendered}");
    assert_eq!(err.class, Class::Unavailable, "{rendered}");
    assert_ne!(err.stage, Stage::Registry, "the stage of the transport stays: {rendered}");
}

/// The class of each status: a missing manifest is `NotFound`, a refused
/// credential `Refused`, a limit of the rate `TempFail`.
#[test]
fn each_status_has_its_class() {
    assert_eq!(class_of(404), Class::NotFound);
    assert_eq!((class_of(401), class_of(403)), (Class::Refused, Class::Refused));
    assert_eq!(class_of(429), Class::TempFail);
    assert_eq!(class_of(503), Class::Unavailable);
    assert_eq!(class_of(400), Class::Protocol);
}

/// A registry named insecure with no trust for it is a failure of the
/// configuration, not a request with no check.
#[test]
fn an_insecure_registry_with_no_trust_is_a_failure_of_the_configuration() {
    pod_testkit::need_loopback!();
    let mut client = client().with_policy(Policy::with_insecure(&["127.0.0.1:1"]));
    let err = match client.manifest("127.0.0.1:1", "library/alpine", "3.20") {
        Ok(_) => panic!("a request with no trust went out"),
        Err(e) => e,
    };
    assert_eq!((err.stage, err.class), (Stage::Tls, Class::Config), "{err}");
}

/// A body whose digest is not the wanted one stops: a new try asks the
/// registry for the same wrong bytes.
#[test]
fn a_wrong_digest_stops_and_is_not_tried_again() {
    pod_testkit::need_loopback!();
    let port = serve_once(b"other bytes".to_vec());
    let resp = get(port);
    let want = Digest::of(b"the wanted bytes");
    match Client::drain(resp, &mut Vec::new(), &want, None, None, "/test") {
        Err(Cut::Stop(e)) => assert!(matches!(crate::refusal(&e), Some(crate::Refusal::Digest { .. })), "{e}"),
        Err(Cut::Again(e)) => panic!("a wrong digest was tried again: {e}"),
        Ok(()) => panic!("a wrong digest was accepted"),
    }
}
