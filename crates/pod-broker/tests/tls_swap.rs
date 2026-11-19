//! The swap of the TLS end (T-100): a placeholder in a header, in Basic, in
//! a query and in a body reaches the far end as the value of its secret,
//! and only for the hosts and the places of its credential.

mod common;
mod tls_common;

use base64::Engine as _;
use common::*;
use tls_common::*;

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn get(target: &str, headers: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nHost: {HOST}\r\n{headers}\r\n").into_bytes()
}

#[tokio::test]
async fn a_placeholder_in_a_header_reaches_the_far_end_as_the_value() {
    let far = TlsFar::new(echo());
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get("/user", &format!("Authorization: Bearer {PH}\r\n"))).await;
    assert_eq!(answer.status, 200);
    assert!(far.seen().contains(&format!("authorization: Bearer {SECRET}")), "{}", far.seen());
    assert!(!far.seen().contains(PH));
    // The far end echoes the value; the job reads the placeholder.
    assert!(text(&answer).contains(&format!("Bearer {PH}")), "{}", text(&answer));
    assert!(!text(&answer).contains(SECRET));
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["tls"].as_str(), line.fields["swaps"].as_u64()), (Some("end"), Some(1)));
    assert!(line.fields["scrubs"].as_u64() >= Some(1), "{line:?}");
}

#[tokio::test]
async fn a_placeholder_in_basic_is_swapped_and_the_echo_of_the_pair_comes_back_as_it_was() {
    let far = TlsFar::new(echo());
    let (broker, _) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let sent = b64(&format!("x-access-token:{PH}"));
    let answer = send(&mut tls, &get("/repo.git/info/refs", &format!("Authorization: Basic {sent}\r\n"))).await;
    let real = b64(&format!("x-access-token:{SECRET}"));
    assert!(far.seen().contains(&format!("authorization: Basic {real}")), "{}", far.seen());
    assert!(text(&answer).contains(&sent) && !text(&answer).contains(&real), "{}", text(&answer));
}

#[tokio::test]
async fn a_placeholder_in_a_query_is_swapped_when_the_credential_takes_the_query() {
    let far = TlsFar::new(echo());
    let (broker, _) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy(r#"["query"]"#, ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get(&format!("/search?q=a&access_token={PH}"), "")).await;
    assert!(far.seen().contains(&format!("GET /search?q=a&access_token={SECRET} HTTP/1.1")), "{}", far.seen());
    assert!(text(&answer).contains(PH) && !text(&answer).contains(SECRET));
}

#[tokio::test]
async fn the_default_places_leave_a_query_and_a_body_alone() {
    let far = TlsFar::new(echo());
    let (broker, _) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let body = format!("{{\"comment\":\"{PH}\"}}");
    let request =
        format!("POST /issues?token={PH} HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    send(&mut tls, request.as_bytes()).await;
    assert!(!far.seen().contains(SECRET), "{}", far.seen());
    assert_eq!(far.seen().matches(PH).count(), 2);
}

#[tokio::test]
async fn a_placeholder_in_a_body_is_swapped_with_a_new_length_and_in_chunks() {
    let far = TlsFar::new(echo());
    let (broker, _) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy(r#"["body"]"#, ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let body = format!("{{\"token\":\"{PH}\"}}");
    let request = format!("POST /x HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let answer = send(&mut tls, request.as_bytes()).await;
    let swapped = format!("{{\"token\":\"{SECRET}\"}}");
    assert!(far.seen().contains(&format!("content-length: {}\r\n", swapped.len())), "{}", far.seen());
    assert!(far.seen().ends_with(&swapped), "{}", far.seen());
    assert!(text(&answer).contains(&body) && !text(&answer).contains(SECRET));
    // The same connection carries a chunked body, the placeholder cut over
    // two chunks.
    let (a, b) = PH.split_at(7);
    let request = format!(
        "POST /y HTTP/1.1\r\nHost: {HOST}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\nk={a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
        a.len() + 2,
        b.len()
    );
    let answer = send(&mut tls, request.as_bytes()).await;
    assert!(far.seen().ends_with(&format!("k={SECRET}")), "{}", far.seen());
    assert!(text(&answer).contains(&format!("k={PH}")) && !text(&answer).contains(SECRET));
}

#[tokio::test]
async fn a_body_over_the_bound_passes_with_no_swap_and_the_audit_line_says_so() {
    let far = TlsFar::new(echo());
    let bounds = pod_broker::tls_end::Bounds { max_body: 64, ..Default::default() };
    let (broker, audit) = tls_broker_with(far.clone(), bounds);
    let ca = load_secret(&broker, &policy(r#"["body"]"#, ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let body = format!("{}{PH}", "x".repeat(100));
    let request = format!("POST /x HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let answer = send(&mut tls, request.as_bytes()).await;
    assert!(far.seen().ends_with(&body) && !far.seen().contains(SECRET), "{}", far.seen());
    // The answer is over the bound too: it comes back in chunks.
    assert_eq!(answer.header("transfer-encoding"), Some("chunked"));
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["notes"], "body-large");
}

#[tokio::test]
async fn a_host_that_no_credential_names_gets_no_swap() {
    // The rules of OTHER each have a path: the TLS end reads its tunnel and
    // checks its paths, and swaps nothing there.
    let allow = format!(r#"[{{"host":"{HOST}"}},{{"host":"{OTHER}","path":"/v1"}},{{"host":"{NO_LEAF}","path":"/"}}]"#);
    let far = TlsFar::new(echo());
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy(r#"["header","basic","query","body"]"#, &allow));
    let mut tls = job_tls(&broker, OTHER, OTHER, &ca).await;
    let body = format!("t={PH}");
    let request = format!(
        "POST /v1/x?t={PH} HTTP/1.1\r\nHost: {OTHER}\r\nAuthorization: Bearer {PH}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let answer = send(&mut tls, request.as_bytes()).await;
    assert_eq!(answer.status, 200, "{}", text(&answer));
    assert_eq!(far.seen().matches(PH).count(), 3, "{}", far.seen());
    assert!(!far.seen().contains(SECRET));
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["tls"].as_str(), line.fields["swaps"].as_u64()), (Some("end"), Some(0)));
    // A far end whose TLS fails: the job gets 502 inside its own TLS.
    let mut tls = job_tls(&broker, NO_LEAF, NO_LEAF, &ca).await;
    let request = format!("GET / HTTP/1.1\r\nHost: {NO_LEAF}\r\nAuthorization: Bearer {PH}\r\n\r\n");
    assert_eq!(send(&mut tls, request.as_bytes()).await.status, 502);
    audit.wait(|l| l.fields["notes"] == "upstream-tls").await;
    assert!(!far.seen().contains(SECRET));
}

#[tokio::test]
async fn a_tunnel_to_a_host_with_no_credential_and_no_path_passes_as_it_is() {
    let far = TlsFar::new(echo());
    let (broker, audit) = tls_broker(far.clone());
    load_secret(&broker, &policy("", ""));
    let (head, mut tunnel) = ask(&broker, &connect("plain.example.com:443")).await;
    assert_eq!(status_of(&head), 200);
    // No TLS end: the bytes of the job reach the far end as they are, and
    // the far end reads them as the start of a handshake.
    use tokio::io::AsyncWriteExt as _;
    tunnel.write_all(format!("Authorization: Bearer {PH}\r\n").as_bytes()).await.unwrap();
    drop(tunnel);
    let line = audit.connection("allow").await;
    assert!(line.fields.get("tls").is_none(), "{line:?}");
    assert!(!far.seen().contains(SECRET));
}

#[tokio::test]
async fn a_path_that_no_rule_allows_is_refused_inside_the_tunnel() {
    let allow = format!(r#"[{{"host":"{HOST}","path":"/repos/owner"}}]"#);
    let far = TlsFar::new(echo());
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", &allow));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get("/repos/owner/x", &format!("Authorization: Bearer {PH}\r\n"))).await;
    assert_eq!(answer.status, 200);
    let answer = send(&mut tls, &get("/repos/other", &format!("Authorization: Bearer {PH}\r\n"))).await;
    assert_eq!(answer.status, 403, "{}", text(&answer));
    assert_eq!(far.seen().matches("GET ").count(), 1, "the refused request never reached the far end");
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["requests"].as_u64(), line.fields["notes"].as_str()), (Some(2), Some("path-denied")));
}

#[tokio::test]
async fn a_host_or_an_sni_that_is_not_the_connect_target_gets_421() {
    let far = TlsFar::new(echo());
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let request = format!("GET / HTTP/1.1\r\nHost: evil.example\r\nAuthorization: Bearer {PH}\r\n\r\n");
    assert_eq!(send(&mut tls, request.as_bytes()).await.status, 421);
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["notes"], "host-mismatch");
    // A job that takes any certificate and asks another name in its
    // handshake gets 421 inside the TLS, and nothing reaches the far end.
    let (head, stream) = ask(&broker, &connect(&format!("{HOST}:443"))).await;
    assert_eq!(status_of(&head), 200);
    let words = pod_tls::Words::new("test");
    let config = any_certificate();
    let tls = pod_tls::tls_over(stream, "evil.example", config, std::time::Duration::from_secs(10), &words).await;
    let mut tls: pod_core::BoxedIo = Box::new(tls.unwrap());
    let request = format!("GET / HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {PH}\r\n\r\n");
    use tokio::io::AsyncWriteExt as _;
    let _ = tls.write_all(request.as_bytes()).await;
    let answer = pod_proxy::http::read_response(&mut tls, Vec::new(), 1 << 20).await.unwrap();
    assert_eq!(answer.status, 421, "{}", text(&answer));
    audit.wait(|l| l.fields["notes"] == "sni-mismatch").await;
    assert!(!far.seen().contains(SECRET));
}

#[tokio::test]
async fn a_tunnel_to_a_host_of_a_credential_that_carries_no_tls_passes_with_no_swap() {
    let end = pod_broker::TlsEnd::new(provider(), trusting(TlsFar::new(echo()).ca.pem()));
    let far = Far::new(Kind::Echo);
    let audit = std::sync::Arc::new(Seen::default());
    let broker = pod_broker::Broker::new(pod_broker::Settings::default(), Answers::public(), far.clone())
        .with_audit(audit.clone())
        .with_tunnel_end(std::sync::Arc::new(end));
    let broker = std::sync::Arc::new(broker);
    load_secret(&broker, &policy("", ""));
    let (head, mut tunnel) = ask(&broker, &connect(&format!("{HOST}:22"))).await;
    assert_eq!(status_of(&head), 200);
    let hello = format!("SSH-2.0-OpenSSH_9.9 {PH}\r\n");
    assert_eq!(round_trip(&mut tunnel, hello.as_bytes()).await, hello.as_bytes());
    drop(tunnel);
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["notes"], "not-tls");
}
