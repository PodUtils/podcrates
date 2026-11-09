//! One resolve: the broker asks the resolver once for each connection,
//! checks each address of the answer, and dials only those addresses. A
//! resolver that changes its answer (DNS rebinding) cannot move the dial.

mod common;

use common::*;
use pod_core::Target;

fn addr(ip: &str, port: u16) -> Target {
    Target::Addr(std::net::SocketAddr::new(ip.parse().unwrap(), port))
}

/// The first answer is public, each later one private: the broker dials
/// the public address, and asks the resolver once.
#[tokio::test]
async fn a_second_answer_of_the_resolver_never_reaches_the_dial() {
    let far = Far::new(Kind::Echo);
    let answers = Answers::new(&[&[PUBLIC], &["10.0.0.7"]]);
    let (broker, _) = broker(far.clone(), answers.clone());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("rebind.example:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(round_trip(&mut tunnel, b"ping").await, b"ping");
    assert_eq!(answers.calls(), 1);
    assert_eq!(far.dialed(), vec![addr(PUBLIC, 443)]);
    // The next connection gets the private answer, and is refused.
    assert_eq!(status(&broker, &connect("rebind.example:443")).await, 403);
    assert_eq!(answers.calls(), 2);
    assert_eq!(far.dialed().len(), 1);
}

/// A name with one denied address among public ones is refused whole.
#[tokio::test]
async fn a_name_with_any_denied_address_is_refused() {
    for denied in ["10.0.0.7", "169.254.169.254", "::ffff:127.0.0.1", "fe80::1", "100.64.1.1"] {
        let far = Far::new(Kind::Echo);
        let answers = Answers::new(&[&[PUBLIC, denied, PUBLIC_TWO]]);
        let (broker, audit) = broker(far.clone(), answers.clone());
        load(&broker, "job1", TOKEN, OPEN);
        assert_eq!(status(&broker, &connect("mixed.example:443")).await, 403, "{denied}");
        assert!(far.dialed().is_empty(), "{denied}");
        let line = audit.lines().into_iter().find(|l| l.event == "broker.connection").unwrap();
        assert!(line.fields["rule"].as_str().unwrap().starts_with("fixed:"), "{denied}: {line:?}");
    }
}

/// Each checked address may be dialed, in the order of the answer, and no
/// other.
#[tokio::test]
async fn the_dial_takes_the_checked_addresses_only() {
    let far = Far::new(Kind::Fail(pod_core::Class::Unavailable));
    let answers = Answers::new(&[&[PUBLIC, PUBLIC_TWO]]);
    let (broker, _) = broker(far.clone(), answers.clone());
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("two.example:8443")).await, 502);
    assert_eq!(far.dialed(), vec![addr(PUBLIC, 8443), addr(PUBLIC_TWO, 8443)]);
    assert_eq!(answers.calls(), 1);
}

/// A literal address is checked as each answer is, and needs no resolve.
#[tokio::test]
async fn a_literal_address_needs_no_resolve() {
    let far = Far::new(Kind::Echo);
    let answers = Answers::public();
    let (broker, _) = broker(far.clone(), answers.clone());
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("198.51.100.20:443")).await, 200);
    assert_eq!(status(&broker, &connect("[::ffff:10.1.1.1]:443")).await, 403);
    assert_eq!(answers.calls(), 0);
    assert_eq!(far.dialed(), vec![addr(PUBLIC_TWO, 443)]);
}

/// A name with no address gets 502, and nothing is dialed.
#[tokio::test]
async fn a_name_with_no_address_gets_502() {
    let far = Far::new(Kind::Echo);
    let (broker, _) = broker(far.clone(), Answers::new(&[&[]]));
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("none.example:443")).await, 502);
    assert!(far.dialed().is_empty());
}

/// The addresses of the host and the deny file of the host hold for each
/// job, also an open one.
#[tokio::test]
async fn the_denies_of_the_host_hold_for_each_job() {
    let far = Far::new(Kind::Echo);
    let deny = pod_broker::DenyList::parse("198.51.100.0/24\nblocked.example\n", "file").unwrap();
    let settings = pod_broker::Settings { deny, host_addresses: vec![PUBLIC.parse().unwrap()], ..Default::default() };
    let (broker, audit) = broker_with(settings, far.clone(), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("any.example:443")).await, 403);
    assert_eq!(status(&broker, &connect("198.51.100.20:443")).await, 403);
    assert_eq!(status(&broker, &connect("blocked.example:443")).await, 403);
    let rules: Vec<String> = audit
        .lines()
        .iter()
        .filter(|l| l.event == "broker.connection")
        .map(|l| l.fields["rule"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(rules, ["host:address", "file:1", "file:2"]);
    assert!(far.dialed().is_empty());
}
