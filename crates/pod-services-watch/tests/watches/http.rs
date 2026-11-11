//! `http-json`, `http-status`, `http-text` and `github-ssh-keys` against an
//! HTTPS stand-in.

use pod_services_watch::Finding;
use pod_testkit::stand::{Answer, Request};

use crate::common::{NAME, endpoint, fact, file, https, one, text};

fn answer(request: &Request) -> Answer {
    match request.path() {
        "/doc.json" => Answer::new(
            200,
            r#"{"version":"r2","limits":{"idle_ms":5,"name":"x"},
                "Regions":{"1":{"Nodes":[{"HostName":"a.test"},{"HostName":"b.test"}]}}}"#,
        ),
        "/health" => Answer::new(200, "ok"),
        "/trace" => Answer::new(200, "fl=1\nh=one.one.one.one\n"),
        "/meta" => Answer::new(
            200,
            r#"{"ssh_key_fingerprints":{"SHA256_ED25519":"k1","SHA256_RSA":"k2"},
                "domains":{"actions_inbound":{"full_domains":["github.com","broker.example.test"]}}}"#,
        ),
        _ => Answer::new(404, "no"),
    }
}

fn watch(lines: &str) -> String {
    format!("[[watch]]\n{lines}\n")
}

#[tokio::test]
async fn http_json_compares_a_fact_and_the_members_of_facts() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let url = format!("https://{NAME}:{}/doc.json", stand.port());
    let body = fact("version", "\"r2\"")
        + &fact("idle", "5")
        + &watch(&format!("kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/version\"\nfact = \"version\""));
    assert!(one(&file("j1", &body)).await.holds());

    let body = fact("version", "\"r1\"")
        + &watch(&format!("kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/version\"\nfact = \"version\""));
    let report = one(&file("j2", &body)).await;
    assert_eq!(report.outcome.findings, vec![Finding::moved("\"version\"", "r1", "\"r2\"")]);
    assert!(text(&report).contains("MOVED j2 http-json: services/j2.toml \"version\" is r1"), "{}", text(&report));

    let body = fact("idle", "6")
        + &watch(&format!(
            "kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/limits\"\nfacts = [\"idle\"]\nmembers = [\"idle_ms\"]"
        ));
    let report = one(&file("j3", &body)).await;
    assert_eq!(report.outcome.findings, vec![Finding::moved("\"idle\"", "6", "5")]);
}

#[tokio::test]
async fn http_json_member_holds_each_host_of_the_role() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let url = format!("https://{NAME}:{}/doc.json", stand.port());
    let body = endpoint("a", "https", "a.test", 443, "role = \"region\"")
        + &endpoint("c", "https", "c.test", 443, "role = \"region\"")
        + &watch(&format!(
            "kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/Regions\"\nmember = \"HostName\"\nrole = \"region\""
        ));
    let report = one(&file("j4", &body)).await;
    let all = text(&report);
    assert_eq!(report.outcome.findings.len(), 1, "{all}");
    assert!(all.contains("the host c.test is an endpoint; the service gives no HostName c.test"), "{all}");
}

#[tokio::test]
async fn http_status_wants_the_status() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let ep = endpoint("api", "https", NAME, stand.port(), "");
    let good = ep.clone() + &watch("kind = \"http-status\"\nendpoint = \"api\"\npath = \"/health\"\nstatus = 200");
    assert!(one(&file("s1", &good)).await.holds());
    let bad = ep + &watch("kind = \"http-status\"\nendpoint = \"api\"\npath = \"/gone\"\nstatus = 200");
    let report = one(&file("s2", &bad)).await;
    assert!(text(&report).contains("answered 404, not 200"), "{}", text(&report));
}

#[tokio::test]
async fn http_text_wants_the_text() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let ep = endpoint("trace", "https", NAME, stand.port(), "path = \"/trace\"");
    let good = ep.clone() + &watch("kind = \"http-text\"\nendpoint = \"trace\"\ncontains = \"h=one.one.one.one\"");
    assert!(one(&file("t1", &good)).await.holds());
    let bad = ep + &watch("kind = \"http-text\"\nendpoint = \"trace\"\ncontains = \"h=two\"");
    let report = one(&file("t2", &bad)).await;
    assert!(!report.holds() && text(&report).contains("does not hold"), "{}", text(&report));
}

#[tokio::test]
async fn github_ssh_keys_are_the_fingerprints_of_the_fact() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let url = format!("https://{NAME}:{}/meta", stand.port());
    let w = watch(&format!(
        "kind = \"github-ssh-keys\"\nurl = \"{url}\"\npointer = \"/ssh_key_fingerprints\"\nfact = \"keys\""
    ));
    let good = fact("keys", "[\"SHA256:k1\", \"SHA256:k2\"]") + &w;
    assert!(one(&file("g1", &good)).await.holds());
    let bad = fact("keys", "[\"SHA256:k1\", \"SHA256:k3\"]") + &w;
    let report = one(&file("g2", &bad)).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved("\"keys\"", "SHA256:k1, SHA256:k3", "SHA256:k1, SHA256:k2")]
    );
}

/// The list of hosts of a self-hosted runner (T-164): the watch holds while
/// the list of the document is the fact, and fails on a host that the
/// document adds or removes.
#[tokio::test]
async fn http_json_fails_on_a_host_that_a_list_adds_or_removes() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let url = format!("https://{NAME}:{}/meta", stand.port());
    let w = watch(&format!(
        "kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/domains/actions_inbound/full_domains\"\nfact = \"hosts\""
    ));
    let same = fact("hosts", r#"["github.com", "broker.example.test"]"#) + &w;
    assert!(one(&file("l1", &same)).await.holds());
    // The document added a host that the file does not hold.
    let added = fact("hosts", r#"["github.com"]"#) + &w;
    let report = one(&file("l2", &added)).await;
    assert!(!report.holds() && text(&report).contains("MOVED l2 http-json"), "{}", text(&report));
    // The document removed a host that the file holds.
    let removed = fact("hosts", r#"["github.com", "broker.example.test", "gone.example.test"]"#) + &w;
    let report = one(&file("l3", &removed)).await;
    assert!(!report.holds() && text(&report).contains("gone.example.test"), "{}", text(&report));
}
