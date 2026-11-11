//! The answers of the broker on the wire: 407, 403, 421, 400, 502 and 504,
//! each with its audit line, and the plain request with a path rule.

mod common;

use common::*;
use pod_broker::Settings;
use pod_core::{Class, Target};

const BLOCKED: &str = r#"{"schema":1,"mode":"allow","allow":[
  {"host":"api.example.com","path":"/v1/repos"},
  {"host":"open.example.com"},
  {"host":"169.254.169.254"},
  {"host":"*.example.net"}
]}"#;

#[tokio::test]
async fn no_token_or_a_token_of_no_job_gets_407() {
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker(far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let target = "example.com:443";
    let bare = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    let (head, _) = ask(&broker, &bare).await;
    assert_eq!(status_of(&head), 407, "{head}");
    assert!(head.contains("Proxy-Authenticate: Basic"), "{head}");
    for auth in [
        basic("job1", "made-up-token-of-no-job"),
        basic("job2", TOKEN),
        "Proxy-Authorization: Digest x\r\n".to_string(),
        "Proxy-Authorization: Basic !!!\r\n".to_string(),
        format!("Proxy-Authorization: Bearer {TOKEN}\r\nProxy-Authorization: Bearer {TOKEN}\r\n"),
    ] {
        let request = format!("CONNECT {target} HTTP/1.1\r\n{auth}\r\n");
        assert_eq!(status(&broker, &request).await, 407, "{auth}");
    }
    let bearer = format!("CONNECT {target} HTTP/1.1\r\nProxy-Authorization: Bearer {TOKEN}\r\n\r\n");
    assert_eq!(status(&broker, &bearer).await, 200);
    assert_eq!(status(&broker, &format!("CONNECT {target} HTTP/1.1\r\n{}\r\n", basic("", TOKEN))).await, 200);
    let lines = audit.lines();
    let refused: Vec<_> = lines.iter().filter(|l| l.fields["status"] == 407).collect();
    assert_eq!(refused.len(), 6);
    assert!(refused.iter().all(|l| l.fields["job"].is_null() && l.class == Some("refused")));
}

#[tokio::test]
async fn metadata_and_private_targets_get_403_in_each_mode() {
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker(far.clone(), Answers::new(&[&["10.0.0.1"]]));
    load(&broker, "job1", TOKEN, BLOCKED);
    load(&broker, "job2", TOKEN_TWO, OPEN);
    for (job, token) in [("job1", TOKEN), ("job2", TOKEN_TWO)] {
        for target in [
            "169.254.169.254:80",
            "168.63.129.16:80",
            "[fd00:ec2::254]:80",
            "[::ffff:169.254.169.254]:80",
            "10.0.0.1:443",
            "127.0.0.1:22",
            "[::1]:22",
            "metadata.google.internal:80",
            "a.localhost:80",
            "private.example.net:443",
            "93.184.215.14:25",
        ] {
            let request = format!("CONNECT {target} HTTP/1.1\r\n{}\r\n", basic(job, token));
            assert_eq!(status(&broker, &request).await, 403, "{job} {target}");
        }
    }
    assert!(far.dialed().is_empty(), "a refused target is never dialed: {:?}", far.dialed());
    let lines = audit.lines();
    let metadata = lines.iter().find(|l| l.fields["host"] == "169.254.169.254").unwrap();
    assert_eq!((&metadata.fields["decision"], &metadata.fields["rule"]), (&"deny".into(), &"fixed:metadata".into()));
    assert_eq!(metadata.class, Some(Class::Policy.name()));
    let named = lines.iter().find(|l| l.fields["host"] == "private.example.net").unwrap();
    assert_eq!((&named.fields["rule"], &named.fields["addr"]), (&"fixed:private".into(), &"10.0.0.1:443".into()));
    let smtp = lines.iter().find(|l| l.fields["port"] == 25).unwrap();
    assert_eq!(smtp.fields["rule"], "fixed:smtp");
}

#[tokio::test]
async fn a_target_that_no_rule_allows_gets_403() {
    let far = Far::new(Kind::Http { answer: b"HTTP/1.1 204 No Content\r\n\r\n".to_vec(), body: 0 });
    let (broker, audit) = broker(far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, BLOCKED);
    let auth = basic("job1", TOKEN);
    let get = |url: &str| format!("GET {url} HTTP/1.1\r\n{auth}\r\n");
    assert_eq!(status(&broker, &get("http://api.example.com/v1/repos/x")).await, 204);
    assert_eq!(status(&broker, &get("http://api.example.com/v1/repos")).await, 204);
    assert_eq!(status(&broker, &get("http://api.example.com/v1/reposx")).await, 403);
    assert_eq!(status(&broker, &get("http://api.example.com/v2")).await, 403);
    assert_eq!(status(&broker, &get("http://other.example.com/")).await, 403);
    assert_eq!(status(&broker, &connect("open.example.com:443")).await, 200);
    // A tunnel shows no path: the rule with a path needs the TLS end of T-100.
    assert_eq!(status(&broker, &connect("api.example.com:443")).await, 403);
    let rules: Vec<String> =
        audit.lines().iter().map(|l| l.fields["rule"].as_str().unwrap_or_default().to_string()).collect();
    assert!(rules.contains(&"policy:no-rule".to_string()) && rules.contains(&"policy:path-needs-tls-end".to_string()));
    assert_eq!(far.dialed().len(), 3);
}

#[tokio::test]
async fn a_host_that_differs_from_the_target_gets_421() {
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker(far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let auth = basic("job1", TOKEN);
    for (line, host) in [
        ("CONNECT a.example:443", "b.example:443"),
        ("CONNECT a.example:443", "a.example:8443"),
        ("GET http://a.example/x", "b.example"),
        ("GET http://a.example/x", "a.example:8080"),
    ] {
        let request = format!("{line} HTTP/1.1\r\nHost: {host}\r\n{auth}\r\n");
        assert_eq!(status(&broker, &request).await, 421, "{line} / {host}");
    }
    for (line, host) in [("CONNECT A.Example.:443", "a.example"), ("CONNECT a.example:443", "a.example:443")] {
        let request = format!("{line} HTTP/1.1\r\nHost: {host}\r\n{auth}\r\n");
        assert_eq!(status(&broker, &request).await, 200, "{line} / {host}");
    }
    assert!(audit.lines().iter().filter(|l| l.fields["status"] == 421).all(|l| l.fields["rule"] == "host-mismatch"));
    assert_eq!(far.dialed().len(), 2);
}

#[tokio::test]
async fn an_ambiguous_target_gets_400() {
    let far = Far::new(Kind::Echo);
    let (broker, _) = broker(far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let auth = basic("job1", TOKEN);
    for line in [
        "CONNECT user@a.example:443",
        "CONNECT a.example",
        "CONNECT a.example:70000",
        "CONNECT 2001:db8::1:443",
        "CONNECT 2130706433:443",
        "CONNECT 0177.0.0.1:443",
        "GET http://a.example/v1/%2e%2e/admin",
        "GET http://a.example/v1//x",
        "GET http://name@a.example/",
        "GET /relative",
        "GET https://a.example/",
    ] {
        let request = format!("{line} HTTP/1.1\r\n{auth}\r\n");
        assert_eq!(status(&broker, &request).await, 400, "{line}");
    }
    let two_hosts = format!("GET http://a.example/ HTTP/1.1\r\nHost: a.example\r\nHost: b.example\r\n{auth}\r\n");
    assert_eq!(status(&broker, &two_hosts).await, 400);
    let smuggled =
        format!("POST http://a.example/ HTTP/1.1\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n{auth}\r\n");
    assert_eq!(status(&broker, &smuggled).await, 400);
    assert_eq!(status(&broker, "CONNECT a.example:443 HTTP/1.1\r\nX: a\rb\r\n\r\n").await, 400);
    assert!(far.dialed().is_empty());
}

#[tokio::test]
async fn a_far_end_that_fails_gets_502() {
    let far = Far::new(Kind::Fail(Class::Unavailable));
    let (broker, audit) = broker(far, Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, _) = ask(&broker, &connect("a.example:443")).await;
    assert_eq!(status_of(&head), 502, "{head}");
    let line = audit.lines().into_iter().find(|l| l.event == "broker.connection").unwrap();
    assert_eq!((line.stage, line.class), (Some("connect"), Some("unavailable")));
    assert_eq!(line.fields["addr"], format!("{PUBLIC}:443"));
}

#[tokio::test(start_paused = true)]
async fn a_far_end_that_does_not_answer_gets_504() {
    let far = Far::new(Kind::Hang);
    let settings = Settings { connect_timeout: std::time::Duration::from_secs(5), ..Settings::default() };
    let (broker, audit) = broker_with(settings, far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("a.example:443")).await, 504);
    assert_eq!(far.dialed(), vec![Target::Addr(format!("{PUBLIC}:443").parse().unwrap())]);
    let line = audit.lines().into_iter().find(|l| l.event == "broker.connection").unwrap();
    assert_eq!(line.class, Some("timeout"));
}

/// A plain request goes in origin form with its own Host, with no
/// credential of the proxy and no hop header, and with its body.
#[tokio::test]
async fn a_plain_request_goes_in_origin_form_with_its_body() {
    let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec();
    let far = Far::new(Kind::Http { answer, body: 15 });
    let (broker, audit) = broker(far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let request = format!(
        "POST http://api.example.com:8080/v1/x?q=1 HTTP/1.1\r\nHost: api.example.com:8080\r\nTransfer-Encoding: chunked\r\n\
         Connection: keep-alive\r\nProxy-Connection: keep-alive\r\n{}\r\n5\r\nhello\r\n0\r\n\r\n",
        basic("job1", TOKEN)
    );
    let (head, mut rest) = ask(&broker, &request).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(read_all(&mut rest).await, "ok");
    let seen = far.seen();
    assert!(seen.starts_with("POST /v1/x?q=1 HTTP/1.1\r\nHost: api.example.com:8080\r\n"), "{seen}");
    assert!(seen.contains("transfer-encoding: chunked\r\n") && seen.ends_with("5\r\nhello\r\n0\r\n\r\n"), "{seen}");
    assert!(seen.contains("Connection: close\r\n") && !seen.to_ascii_lowercase().contains("proxy-"), "{seen}");
    assert!(!seen.contains("keep-alive"), "{seen}");
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["kind"].as_str(), line.fields["decision"].as_str()), (Some("http"), Some("allow")));
    assert_eq!(line.fields["down"], 40);
}
