//! `dns-srv`, `dns-addresses` and `doh-answer` against a stand-in of DNS
//! over HTTPS (the JSON form), and `stun-binding` against a UDP stand-in.

use std::net::IpAddr;
use std::sync::Arc;

use pod_core::{BoxFuture, Connected, Dial, Failure, Target};
use tokio::time::Instant;

use pod_dns::Doh;
use pod_proxy::stun;
use pod_services_watch::Finding;
use pod_testkit::stand::{self, Answer, Request};

use crate::common::{NAME, endpoint, fact, file, https, net, one, one_in, text};

/// The JSON answers of the stand-in: an SRV of two targets, and A records.
fn answer(request: &Request) -> Answer {
    let target = &request.target;
    let body = if target.contains("type=SRV") {
        r#"{"Status":0,"Answer":[{"type":33,"TTL":60,"data":"1 1 7844 region1.edge.test."},
            {"type":33,"TTL":60,"data":"1 1 7844 region2.edge.test."}]}"#
    } else if target.contains("name=none.test") || target.contains("type=AAAA") {
        r#"{"Status":0,"Answer":[]}"#
    } else {
        r#"{"Status":0,"Answer":[{"type":1,"TTL":60,"data":"192.0.2.7"},{"type":1,"TTL":60,"data":"192.0.2.8"}]}"#
    };
    Answer::new(200, body).header("Content-Type", "application/dns-json")
}

/// A dial whose far end resolves names, so that a resolver is asked by its
/// name, which the test certificate holds, and not by its address.
struct ByName(Arc<dyn Dial>);

impl Dial for ByName {
    fn label(&self) -> String {
        "by name".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        self.0.dial(target, deadline)
    }

    fn names_inside(&self) -> bool {
        true
    }
}

/// A way out that asks the resolvers by name.
fn by_name() -> pod_services_watch::Net {
    let mut net = net();
    net.dial = Arc::new(ByName(net.dial.clone()));
    net
}

/// A way out whose resolver of DNS over HTTPS is the stand-in.
fn net_with(port: u16) -> pod_services_watch::Net {
    let mut net = by_name();
    net.dns.resolvers = vec![Doh::new(IpAddr::from([127, 0, 0, 1]), NAME).port(port).json("/dns-query")];
    net
}

fn edges() -> String {
    endpoint("r1", "tls", "region1.edge.test", 7844, "role = \"h2\"")
        + &endpoint("r2", "tls", "region2.edge.test", 7844, "role = \"h2\"")
}

#[tokio::test]
async fn dns_srv_names_the_hosts_and_the_port_of_the_role() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let w = "[[watch]]\nkind = \"dns-srv\"\nfact = \"srv\"\nrole = \"h2\"\n";
    let good = edges() + &fact("srv", "\"_edge._tcp.edge.test\"") + w;
    assert!(one_in(&mut net_with(stand.port()), &file("d1", &good)).await.holds());
    let bad = edges()
        + &endpoint("r3", "tls", "region3.edge.test", 7844, "role = \"h2\"")
        + &fact("srv", "\"_edge._tcp.edge.test\"")
        + w;
    let report = one_in(&mut net_with(stand.port()), &file("d2", &bad)).await;
    assert!(matches!(&report.outcome.findings[..], [Finding::Moved { .. }]), "{}", text(&report));
}

#[tokio::test]
async fn dns_addresses_hold_each_address_of_the_facts() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let w = "[[watch]]\nkind = \"dns-addresses\"\nrole = \"h2\"\nfacts = [\"fixed\", \"probe\"]\n";
    let good = edges() + &fact("fixed", "[\"192.0.2.7\", \"192.0.2.8\"]") + &fact("probe", "\"192.0.2.7\"") + w;
    assert!(one_in(&mut net_with(stand.port()), &file("d3", &good)).await.holds());
    let bad = edges() + &fact("fixed", "[\"192.0.2.7\", \"192.0.2.9\"]") + &fact("probe", "\"192.0.2.7\"") + w;
    let report = one_in(&mut net_with(stand.port()), &file("d4", &bad)).await;
    assert!(
        matches!(&report.outcome.findings[..], [Finding::Moved { was, .. }] if was == "192.0.2.9"),
        "{}",
        text(&report)
    );
}

#[tokio::test]
async fn doh_answer_asks_each_resolver_of_the_service() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let ep = endpoint("v4-1", "https", NAME, stand.port(), "role = \"v4\"\naddresses = [\"127.0.0.1\"]");
    let good = ep.clone()
        + &fact("json-path", "\"/dns-query\"")
        + "[[watch]]\nkind = \"doh-answer\"\nform = \"json\"\nname = \"example.test\"\ntype = \"A\"\n";
    assert!(one_in(&mut by_name(), &file("d5", &good)).await.holds());
    let bad = ep
        + &fact("json-path", "\"/dns-query\"")
        + "[[watch]]\nkind = \"doh-answer\"\nform = \"json\"\nname = \"none.test\"\ntype = \"A\"\n";
    let report = one_in(&mut by_name(), &file("d6", &bad)).await;
    assert!(!report.holds() && text(&report).contains("127.0.0.1"), "{}", text(&report));
}

/// The stand-in answers with the transaction of the request, or with
/// another one when `wrong`.
fn stun_stand(wrong: bool) -> stand::Stand {
    stand::udp(move |datagram| {
        let request = stun::parse(datagram).ok()?;
        let mut id = request.id;
        if wrong {
            id[0] ^= 0xff;
        }
        Some(stun::binding_success(&id, "198.51.100.4:4000".parse().unwrap()))
    })
    .expect("a UDP stand-in")
}

#[tokio::test]
async fn stun_binding_wants_a_success_of_the_same_transaction() {
    pod_testkit::need_loopback!();
    let w = "[[watch]]\nkind = \"stun-binding\"\nendpoint = \"udp\"\n";
    let good = stun_stand(false);
    let report = one(&file("u1", &(endpoint("udp", "stun", "127.0.0.1", good.port(), "") + w))).await;
    assert!(report.holds() && text(&report).contains("198.51.100.4:4000"), "{}", text(&report));
    let bad = stun_stand(true);
    let report = one(&file("u2", &(endpoint("udp", "stun", "127.0.0.1", bad.port(), "") + w))).await;
    assert!(text(&report).contains("did not answer"), "{}", text(&report));
}
