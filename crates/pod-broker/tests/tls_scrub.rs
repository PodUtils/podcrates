//! The scrub of the TLS end (T-100): an answer that echoes the value of a
//! secret, in a header, in a body by its length, in chunks or in base64,
//! reaches the job with the placeholder; an answer that
//! the scrub cannot read is refused. A planted secret is in no audit line,
//! no line of `tracing`, no answer to the job and no error text.

mod common;
mod tls_common;

use std::sync::Arc;

use base64::Engine as _;
use common::*;
use tls_common::*;

fn get(target: &str, headers: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nHost: {HOST}\r\n{headers}\r\n").into_bytes()
}

/// Each form of the secret that must never reach the job.
fn forms() -> Vec<String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    vec![SECRET.to_string(), b64.encode(SECRET), b64.encode(format!("x:{SECRET}"))]
}

fn assert_clean(what: &str, text: &str) {
    for form in forms() {
        assert!(!text.contains(&form), "{what} holds the secret: {text}");
    }
}

#[tokio::test]
async fn an_answer_that_echoes_the_value_in_each_form_comes_back_with_the_placeholder() {
    let answer: Answer = Arc::new(|_: &str, _: &[u8]| {
        let b64 = base64::engine::general_purpose::STANDARD;
        let body = format!("raw={SECRET} b64={}", b64.encode(SECRET));
        ok(&format!("X-Token: {SECRET}\r\n"), body.as_bytes())
    });
    let far = TlsFar::new(answer);
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get("/", "")).await;
    let b64 = base64::engine::general_purpose::STANDARD;
    assert_eq!(answer.header("x-token"), Some(PH));
    assert_eq!(answer.text(), format!("raw={PH} b64={}", b64.encode(PH)));
    // The new length is the one of the scrubbed body.
    assert_eq!(answer.header("content-length"), Some(answer.body.len().to_string().as_str()));
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["scrubs"].as_u64(), Some(3), "{line:?}");
}

#[tokio::test]
async fn a_chunked_answer_and_an_answer_over_the_bound_are_scrubbed_as_they_come() {
    let answer: Answer = Arc::new(|head: &str, _: &[u8]| {
        if head.starts_with("GET /chunked") {
            let (a, b) = SECRET.split_at(10);
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\nv={a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                a.len() + 2,
                b.len()
            )
            .into_bytes()
        } else {
            ok("", format!("{}{SECRET}{}", "a".repeat(5000), "b".repeat(5000)).as_bytes())
        }
    });
    let far = TlsFar::new(answer);
    let bounds = pod_broker::tls_end::Bounds { max_body: 1024, ..Default::default() };
    let (broker, _) = tls_broker_with(far.clone(), bounds);
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get("/chunked", "")).await;
    assert_eq!(answer.text(), format!("v={PH}"));
    // One connection carries the next request: the answer over the bound
    // comes back in chunks, scrubbed.
    let answer = send(&mut tls, &get("/large", "")).await;
    assert_eq!(answer.header("transfer-encoding"), Some("chunked"));
    assert_eq!(answer.text(), format!("{}{PH}{}", "a".repeat(5000), "b".repeat(5000)));
}

#[tokio::test]
async fn an_answer_with_a_coding_is_refused_and_the_request_asked_for_none() {
    let answer: Answer = Arc::new(|_: &str, _: &[u8]| ok("Content-Encoding: gzip\r\n", b"\x1f\x8b compressed"));
    let far = TlsFar::new(answer);
    let (broker, audit) = tls_broker(far.clone());
    let ca = load_secret(&broker, &policy("", ""));
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let answer = send(&mut tls, &get("/", "Accept-Encoding: gzip, br\r\n")).await;
    assert_eq!(answer.status, 502, "{}", text(&answer));
    assert!(far.seen().contains("accept-encoding: identity\r\n") && !far.seen().contains("gzip"), "{}", far.seen());
    drop(tls);
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["notes"], "answer-encoded");
}

/// The planted secret goes through each path of the TLS end: a swap in
/// each place, an echo, a refusal of the path, of the Host and of a coding,
/// a far end whose TLS fails, a load that fails and the control. No audit
/// line, no line of `tracing`, no answer and no error text holds it.
#[tokio::test]
async fn a_planted_secret_is_in_no_log_line_no_audit_line_no_answer_and_no_error() {
    let logs = Logs::default();
    let _guard = tracing::subscriber::set_default(logs.clone());
    let answer: Answer = Arc::new(|head: &str, body: &[u8]| {
        if head.contains("/coded") {
            return ok("Content-Encoding: br\r\n", b"x");
        }
        let mut all = head.as_bytes().to_vec();
        all.extend_from_slice(body);
        ok(&format!("X-Echo: {SECRET}\r\n"), &all)
    });
    let far = TlsFar::new(answer);
    let (broker, audit) = tls_broker(far.clone());
    let allow = format!(r#"[{{"host":"{HOST}","path":"/ok"}},{{"host":"{NO_LEAF}","path":"/"}}]"#);
    let ca = load_secret(&broker, &policy(r#"["header","basic","query","body"]"#, &allow));
    let mut seen_by_job = String::new();
    let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
    let b64 = base64::engine::general_purpose::STANDARD;
    let body = format!("t={PH}");
    let requests = [
        get(&format!("/ok?t={PH}"), &format!("Authorization: Basic {}\r\n", b64.encode(format!("x:{PH}")))),
        format!("POST /ok HTTP/1.1\r\nHost: {HOST}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes(),
        get("/ok/coded", &format!("X-Key: {PH}\r\n")),
    ];
    for request in requests {
        seen_by_job.push_str(&text(&send(&mut tls, &request).await));
    }
    for request in [
        get("/denied", &format!("X-Key: {PH}\r\n")),
        format!("GET /ok HTTP/1.1\r\nHost: x.example\r\nX-Key: {PH}\r\n\r\n").into_bytes(),
    ] {
        let mut tls = job_tls(&broker, HOST, HOST, &ca).await;
        seen_by_job.push_str(&text(&send(&mut tls, &request).await));
    }
    // A far end whose TLS fails: its error text goes to the job.
    let mut tls = job_tls(&broker, NO_LEAF, NO_LEAF, &ca).await;
    let request = format!("GET / HTTP/1.1\r\nHost: {NO_LEAF}\r\nX-Key: {PH}\r\n\r\n");
    seen_by_job.push_str(&text(&send(&mut tls, request.as_bytes()).await));
    drop(tls);
    assert!(far.seen().contains(SECRET), "the far end got the value: {}", far.seen());
    assert!(seen_by_job.contains("502") && seen_by_job.contains("403") && seen_by_job.contains("421"), "{seen_by_job}");
    // A load that fails, and the control, with the secret in its request.
    let bad = format!(
        r#"{{"schema":1,"mode":"open","credentials":[{{"placeholder":"{SECRET}","secret":"s","hosts":["a.example"]}}]}}"#
    );
    let line = format!(
        r#"{{"op":"load","job":"job2","token":"{TOKEN_TWO}","ttl_s":60,"policy":{bad},"secrets":{{"s":"{SECRET}"}}}}"#
    );
    let mut control = broker.control_line(&line).to_string();
    let good = format!(
        r#"{{"op":"load","job":"job3","token":"made-up-token-of-job-3","ttl_s":60,"policy":{},"secrets":{{"github":"{SECRET}"}}}}"#,
        policy("", "")
    );
    control.push_str(&broker.control_line(&good).to_string());
    control.push_str(&broker.control_line(r#"{"op":"list"}"#).to_string());
    control.push_str(
        &broker
            .control_line(r#"{"op":"load","job":"j","token":"x","ttl_s":1,"policy":{},"secrets":{"a":1}}"#)
            .to_string(),
    );
    assert!(control.contains(r#""ok":true"#) && control.contains("ca_pem"), "{control}");
    audit.wait(|l| l.fields["notes"] == "upstream-tls").await;
    let lines: Vec<String> = audit.lines().iter().map(|l| l.to_json()).collect();
    assert!(lines.len() >= 7, "{lines:?}");
    assert_clean("an answer to the job", &seen_by_job);
    assert_clean("an audit line", &lines.join("\n"));
    assert_clean("a line of tracing", &logs.text());
    assert_clean("an answer of the control", &control);
    assert_clean("the debug form of a vault", &format!("{:?}", pod_broker::Secrets::new().with("github", SECRET)));
}
