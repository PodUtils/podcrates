//! A target's name pinned to loopback addresses goes with no proxy, as a
//! loopback name does: the proxy's loopback is not this host's. The pins
//! are the resolver's that the caller gives the dialer.

use std::ffi::OsString;
use std::sync::Arc;

use pod_proxy::resolve::System;
use pod_proxy::{Dialer, Environment, ProxyChoice, Settings};

#[test]
fn a_name_pinned_to_loopback_alone_goes_with_no_proxy() {
    let proxy = |name: &str| (name == "https_proxy").then(|| OsString::from("http://proxy.example:3128"));
    let pins = System::with_pins(vec![
        ("relay-a.test".into(), "127.0.0.1".parse().unwrap()),
        ("relay-b.test".into(), "203.0.113.5".parse().unwrap()),
        ("relay-c.test".into(), "::1".parse().unwrap()),
        ("relay-c.test".into(), "127.0.0.2".parse().unwrap()),
        ("relay-d.test".into(), "127.0.0.1".parse().unwrap()),
        ("relay-d.test".into(), "203.0.113.5".parse().unwrap()),
    ]);
    let settings = Settings::new("podssh", ProxyChoice::FromEnvironment(Environment::read(&proxy)));
    let dialer = Dialer::new(settings, Arc::new(pins));
    assert_eq!(dialer.proxy_for("relay-a.test").unwrap(), None, "pinned to 127.0.0.1");
    assert_eq!(dialer.proxy_for("RELAY-C.test.").unwrap(), None, "pinned to ::1 and 127.0.0.2");
    let kept = |host: &str| dialer.proxy_for(host).unwrap().map(|p| p.host);
    assert_eq!(kept("relay-b.test").as_deref(), Some("proxy.example"), "a pin to another address");
    assert_eq!(kept("relay-d.test").as_deref(), Some("proxy.example"), "one pin not on loopback");
    assert_eq!(kept("relay-e.test").as_deref(), Some("proxy.example"), "no pin");
}
