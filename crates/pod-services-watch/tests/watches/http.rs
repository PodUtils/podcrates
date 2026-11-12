//! `http-json` (each form), `http-status`, `http-text` and `github-ssh-keys`
//! against an HTTPS stand-in.

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
        "/nodes.json" => Answer::new(200, NODES),
        "/relays.json" => Answer::new(200, RELAYS),
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

/// A list of nodes in the form of the list of Tox: K1, K2 and K5 have 443
/// and `status_tcp`; K2 writes its IPv6 address in a long form, K4 its
/// port as a number.
const NODES: &str = r#"{"nodes":[
    {"ipv4":"192.0.2.1","ipv6":"-","tcp_ports":[33445,443],"public_key":"K1","status_tcp":true},
    {"ipv4":"192.0.2.2","ipv6":"2001:DB8:0:0::2","tcp_ports":[443],"public_key":"K2","status_tcp":true},
    {"ipv4":"192.0.2.3","ipv6":"-","tcp_ports":[3389],"public_key":"K3","status_tcp":true},
    {"ipv4":"192.0.2.4","ipv6":"-","tcp_ports":443,"public_key":"K4","status_tcp":false},
    {"ipv4":"192.0.2.5","ipv6":"-","tcp_ports":[443],"public_key":"K5","status_tcp":true}]}"#;

/// The watch of the form `key` on [`NODES`].
fn items_watch(port: u16) -> String {
    let url = format!("https://{NAME}:{port}/nodes.json");
    watch(&format!(
        "kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/nodes\"\nkey = \"public_key\"\n\
         fact-prefix = \"key-\"\nmember = [\"ipv4\", \"ipv6\"]\nport-member = \"tcp_ports\"\n\
         true-members = [\"status_tcp\"]"
    ))
}

/// An endpoint on 443 and its fact `key-NAME`.
fn node(name: &str, host: &str, key: &str) -> String {
    endpoint(name, "tcp", host, 443, "") + &fact(&format!("key-{name}"), &format!("\"{key}\""))
}

#[tokio::test]
async fn http_json_items_find_each_endpoint_by_its_key() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let good = node("a", "192.0.2.1", "K1") + &node("b", "2001:db8::2", "K2") + &items_watch(stand.port());
    let report = one(&file("i1", &good)).await;
    let all = text(&report);
    assert!(report.holds(), "{all}");
    // K5 fits too, and the file does not have it: counted, not found.
    assert!(all.contains("3 of 5 items with tcp_ports 443 and status_tcp true, 1 of them not in the file"), "{all}");

    let moved = node("a", "192.0.2.1", "K1") + &node("b", "192.0.2.2", "K9") + &items_watch(stand.port());
    let report = one(&file("i2", &moved)).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved("\"key-b\"", "K9", "no item with the public_key K9 at /nodes")]
    );

    let no_fact = endpoint("c", "tcp", "192.0.2.1", 443, "") + &items_watch(stand.port());
    let report = one(&file("i3", &no_fact)).await;
    assert!(text(&report).contains("FAILED i3 http-json: the service has no fact \"key-c\""), "{}", text(&report));
}

/// A pool in the form of Syncthing's: each relay is a URL whose query holds
/// its device ID. R2 is on another port, R3 an IPv6 address, R4 has an
/// escape in its query.
const RELAYS: &str = r#"{"relays":[
    {"url":"relay://192.0.2.1:443/?id=R1&pingInterval=1m0s"},
    {"url":"relay://192.0.2.2:22067/?id=R2"},
    {"url":"relay://[2001:DB8::3]:443/?id=R3"},
    {"url":"relay://192.0.2.4:443/?providedBy=a%20b+c&id=R4"},
    {"url":"relay://192.0.2.5:443/?id=R5"},
    {"stats":{}}]}"#;

/// The watch of the form `key` with `url-member` on [`RELAYS`].
fn relays_watch(port: u16) -> String {
    let url = format!("https://{NAME}:{port}/relays.json");
    watch(&format!(
        "kind = \"http-json\"\nurl = \"{url}\"\npointer = \"/relays\"\nurl-member = \"url\"\n\
         key = \"id\"\nfact-prefix = \"id-\"\nmember = \"host\"\nport-member = \"port\""
    ))
}

/// An endpoint on 443 and its fact `id-NAME`.
fn relay(name: &str, host: &str, id: &str) -> String {
    endpoint(name, "tcp", host, 443, "") + &fact(&format!("id-{name}"), &format!("\"{id}\""))
}

#[tokio::test]
async fn http_json_items_read_the_host_the_port_and_the_query_of_a_url() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let good = relay("a", "192.0.2.1", "R1")
        + &relay("c", "2001:db8::3", "R3")
        + &relay("d", "192.0.2.4", "R4")
        + &relays_watch(stand.port());
    let report = one(&file("u1", &good)).await;
    let all = text(&report);
    assert!(report.holds(), "{all}");
    // R5 is on 443 too, and the file does not have it: counted, not found.
    assert!(all.contains("4 of 6 items with port 443, 1 of them not in the file"), "{all}");

    let moved = relay("b", "192.0.2.2", "R2") + &relay("e", "192.0.2.50", "R5") + &relays_watch(stand.port());
    let report = one(&file("u2", &moved)).await;
    assert_eq!(
        report.outcome.findings,
        vec![
            Finding::moved("endpoint b", "port 443", "port 22067"),
            Finding::moved("endpoint e", "192.0.2.50", "host \"192.0.2.5\""),
        ]
    );
}

#[tokio::test]
async fn http_json_items_hold_the_host_the_port_and_the_true_members() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let body = node("c", "192.0.2.3", "K3")
        + &node("d", "192.0.2.4", "K4")
        + &node("e", "192.0.2.50", "K5")
        + &items_watch(stand.port());
    let report = one(&file("i4", &body)).await;
    assert_eq!(
        report.outcome.findings,
        vec![
            Finding::moved("endpoint c", "port 443", "tcp_ports [3389]"),
            Finding::moved("endpoint d", "status_tcp true", "status_tcp false"),
            Finding::moved("endpoint e", "192.0.2.50", "ipv4 \"192.0.2.5\", ipv6 \"-\""),
        ]
    );
    let all = text(&report);
    assert!(all.contains("MOVED i4 http-json: services/i4.toml endpoint d is status_tcp true"), "{all}");
    assert!(all.contains("(source: measured: a test, read 2026-11-10)"), "{all}");
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

/// `http-text` reads its URL, where the service of the file has no
/// endpoint, as a relay of SSH whose document is on another host; or the
/// URL of an endpoint.
#[tokio::test]
async fn http_text_wants_the_text() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let url = format!("https://{NAME}:{}/trace", stand.port());
    let w = |contains: &str| watch(&format!("kind = \"http-text\"\nurl = \"{url}\"\ncontains = \"{contains}\""));
    let good = fact("zone", "\"one.one.one.one\"") + &w("h=one.one.one.one");
    let report = one(&file("t1", &good)).await;
    assert!(report.holds() && text(&report).contains("/trace holds \"h=one.one.one.one\""), "{}", text(&report));
    let bad = fact("zone", "\"one.one.one.one\"") + &w("h=two");
    let report = one(&file("t2", &bad)).await;
    assert!(!report.holds() && text(&report).contains("/trace does not hold \"h=two\""), "{}", text(&report));
    // A URL with no answer of 2xx is a failure, not a pass.
    let gone = fact("zone", "\"one.one.one.one\"")
        + &watch(&format!("kind = \"http-text\"\nurl = \"https://{NAME}:{}/gone\"\ncontains = \"no\"", stand.port()));
    let report = one(&file("t3", &gone)).await;
    assert!(!report.holds() && text(&report).contains("answered HTTP 404"), "{}", text(&report));
    // The URL of an endpoint of the service, as a document of the service.
    let ep = endpoint("doc", "https", NAME, stand.port(), "path = \"/trace\"\n");
    let by_endpoint = ep + &watch("kind = \"http-text\"\nendpoint = \"doc\"\ncontains = \"h=one.one.one.one\"");
    let report = one(&file("t4", &by_endpoint)).await;
    assert!(report.holds() && text(&report).contains("/trace holds"), "{}", text(&report));
    let neither = fact("zone", "\"one.one.one.one\"") + &watch("kind = \"http-text\"\ncontains = \"h=\"");
    let report = one(&file("t5", &neither)).await;
    assert!(text(&report).contains("the arguments: no argument of url or endpoint"), "{}", text(&report));
    // The arguments are checked before the network: an unknown argument and
    // a name of no endpoint fail by name (T-324).
    let bad = fact("zone", "\"one.one.one.one\"")
        + &watch("kind = \"http-text\"\nendpoint = \"nope\"\ncontains = \"h=\"\nurl-of = \"x\"");
    let all = text(&one(&file("t6", &bad)).await);
    assert!(all.contains("an unknown argument \"url-of\"") && all.contains("no endpoint \"nope\""), "{all}");
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
