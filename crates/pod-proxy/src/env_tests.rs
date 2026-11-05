use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::proxy::{Carrier, ProxyKind};

fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| OsString::from(v))
}

/// The rules of a consumer that reads the upper case first, passes over a
/// URL that cannot be used, takes 8080 for an `http://` URL with no port,
/// the ports of `NO_PROXY`, and sends the loopback through the proxy.
fn podnet() -> Rules {
    Rules {
        proxy_vars: &PROXY_VARS_UPPER_FIRST,
        no_proxy_vars: &NO_PROXY_VARS_UPPER_FIRST,
        skip_unusable: true,
        http_port: 8080,
        no_proxy_ports: true,
        loopback_direct: false,
    }
}

#[test]
fn the_environment_keeps_the_variable_of_each_value() {
    let env = Environment::read(&vars(&[("HTTPS_PROXY", "http://proxy.example:3128"), ("no_proxy", "10.0.0.0/8")]));
    let proxy = env.proxy.as_ref().expect("a proxy");
    assert_eq!(proxy.origin, Origin::Variable("HTTPS_PROXY".into()));
    assert_eq!(proxy.value.as_ref().map(|p| p.port), Ok(3128));
    assert_eq!(env.no_proxy.as_ref().map(|l| l.origin.to_string()).as_deref(), Some("no_proxy"));
    assert_eq!(env.no_proxy_entry("10.1.2.3"), Some("10.0.0.0/8"));
    assert_eq!(env.proxy_for("10.1.2.3", &[]), Ok(None));
    assert!(env.proxy_for("relay.example", &[]).unwrap().is_some());
    assert_eq!(Environment::read(&vars(&[])), Environment::default());
}

/// A URL that cannot be used is kept with why, and a target that goes
/// directly never meets it.
#[test]
fn a_bad_url_is_an_error_only_for_a_target_that_takes_the_proxy() {
    let env = Environment::read(&vars(&[("ALL_PROXY", "ftp://proxy.example"), ("NO_PROXY", "example.com")]));
    assert!(env.proxy_for("relay.test", &[]).unwrap_err().contains("not supported"));
    assert_eq!(env.proxy_for("relay.example.com", &[]), Ok(None));
    assert_eq!(env.proxy_for("localhost", &[]), Ok(None));
}

/// A value that is not UTF-8 counts as unset, as `std::env::var` reads
/// it.
#[cfg(unix)]
#[test]
fn a_value_that_is_not_utf8_is_unset() {
    use std::os::unix::ffi::OsStringExt;
    let var = |name: &str| match name {
        "https_proxy" => Some(OsString::from_vec(vec![0xff, 0xfe])),
        "HTTPS_PROXY" => Some(OsString::from("http://second.example:8080")),
        _ => None,
    };
    let env = Environment::read(&var);
    assert_eq!(env.proxy.map(|p| p.origin), Some(Origin::Variable("HTTPS_PROXY".into())));
}

#[test]
fn no_proxy_rules() {
    let list = NoProxy::parse("127.0.0.1, .example.com,internal:8443 10.0.0.0/8,[::1],*.corp");
    assert!(list.matches("127.0.0.1", 80));
    assert!(!list.matches("127.0.0.2", 80));
    assert!(list.matches("example.com", 443));
    assert!(list.matches("api.example.com", 443));
    assert!(!list.matches("badexample.com", 443));
    assert!(list.matches("internal", 8443));
    assert!(!list.matches("internal", 443));
    assert!(list.matches("10.20.30.40", 1));
    assert!(!list.matches("11.0.0.1", 1));
    assert!(list.matches("::1", 1));
    assert!(list.matches("[::1]", 1));
    assert!(list.matches("a.corp", 1));
    assert!(NoProxy::parse("*").matches("anything", 1));
    assert!(!NoProxy::default().matches("anything", 1));
}

/// The rules of podnet: the upper case first, a URL that cannot be used
/// passed over to the next variable, the port 8080, a port of `NO_PROXY`,
/// and the loopback through the proxy.
#[test]
fn the_rules_of_a_consumer_choose_the_order_the_ports_and_the_loopback() {
    let both = vars(&[("https_proxy", "http://lower.example:1"), ("HTTPS_PROXY", "http://upper.example")]);
    assert_eq!(proxy_from_vars("relay.example", &both).unwrap().unwrap().host, "lower.example");
    let env = Environment::read_with(&both, podnet());
    let upper = env.proxy_at("relay.example", 443, &[]).unwrap().unwrap();
    assert_eq!((upper.host.as_str(), upper.port), ("upper.example", 8080));

    let skipping = vars(&[("HTTPS_PROXY", "ftp://bad.example"), ("ALL_PROXY", "socks5://good.example")]);
    let env = Environment::read_with(&skipping, podnet());
    let good = env.proxy.as_ref().expect("the next variable").value.clone().unwrap();
    assert_eq!((good.host.as_str(), good.kind), ("good.example", ProxyKind::Socks5 { remote_dns: false }));
    assert_eq!(env.skipped.len(), 1);
    assert_eq!(env.skipped[0].origin, Origin::Variable("HTTPS_PROXY".into()));
    assert!(env.skipped[0].value.contains("ftp://"), "{:?}", env.skipped);
    let kept = Environment::read(&skipping);
    assert!(kept.proxy_for("relay.example", &[]).is_err() && kept.skipped.is_empty());

    let ports = vars(&[("HTTPS_PROXY", "http://proxy.example:3128"), ("NO_PROXY", "internal:8443")]);
    let env = Environment::read_with(&ports, podnet());
    assert_eq!(env.proxy_at("internal", 8443, &[]), Ok(None));
    assert!(env.proxy_at("internal", 443, &[]).unwrap().is_some());
    assert_eq!(env.proxy_for("internal", &[]), Ok(None), "no port known: the port of the entry is passed over");
    let env = Environment::read(&ports);
    assert_eq!(env.proxy_at("internal", 443, &[]), Ok(None), "the port of an entry is passed over");

    let loopback = vars(&[("HTTPS_PROXY", "http://proxy.example:3128")]);
    let env = Environment::read_with(&loopback, podnet());
    assert!(env.proxy_at("localhost", 80, &[]).unwrap().is_some());
    let pinned = ["127.0.0.1".parse().unwrap()];
    assert!(env.proxy_at("pinned.test", 80, &pinned).unwrap().is_some());
    assert_eq!(Environment::read(&loopback).proxy_at("pinned.test", 80, &pinned), Ok(None));
}

/// A value that the user gave on purpose takes the place of the variables'
/// proxy, can be a command, and keeps the `NO_PROXY` list; from a variable
/// a command is refused.
#[test]
fn a_proxy_of_the_user_can_be_a_command_and_keeps_no_proxy() {
    let env = Environment::read_with(
        &vars(&[("HTTPS_PROXY", "http://variable.example:3128"), ("NO_PROXY", ".internal")]),
        podnet(),
    );
    let user = env.with_proxy("http+exec:nc gw 3128", Origin::User("--proxy".into()));
    let given = user.proxy.as_ref().unwrap();
    assert_eq!(given.origin.to_string(), "--proxy");
    assert!(matches!(given.value.as_ref().unwrap().carrier(), Carrier::Exec(c) if c == "nc gw 3128"));
    assert_eq!(user.proxy_at("db.internal", 5432, &[]), Ok(None));
    assert_eq!(user.decide("relay.example", Some(443), &[]).1, "exec");

    let refused = Environment::read(&vars(&[("ALL_PROXY", "http+exec:nc gw 3128")]));
    assert!(refused.proxy_for("relay.example", &[]).unwrap_err().contains("+exec:"));
    let file = Environment::default().with_proxy("http+exec:nc gw", Origin::File("/etc/environment".into()));
    assert!(file.proxy.unwrap().value.is_err(), "a file never names a command");
}

/// With no variable, the proxy of the first tool's file that names one,
/// with the file as its origin; `HOME` comes from the caller's reader. A
/// variable wins over a file, and a file's proxy that cannot be used is
/// passed over.
#[test]
fn the_proxy_of_a_tool_comes_through_the_readers_of_the_caller() {
    let home = vars(&[("HOME", "/home/u")]);
    let files = |path: &Path| match path.to_str() {
        Some("/home/u/.npmrc") => Some("https-proxy=http://npm.example:3128\n".to_string()),
        Some("/etc/environment") => Some("https_proxy=http://etc.example:3128\n".to_string()),
        _ => None,
    };
    let env = Environment::read(&home).or_detected(&home, &files);
    let given = env.proxy.as_ref().unwrap();
    assert_eq!(given.origin, Origin::File(PathBuf::from("/home/u/.npmrc")));
    assert_eq!(given.value.as_ref().unwrap().host, "npm.example");
    assert_eq!(env.decide("relay.example", Some(443), &[]).1, "http");

    let no_home = Environment::read(&vars(&[])).or_detected(&vars(&[]), &files);
    assert_eq!(no_home.proxy.unwrap().origin, Origin::File(PathBuf::from("/etc/environment")));

    let set = vars(&[("HOME", "/home/u"), ("ALL_PROXY", "socks5h://var.example")]);
    let env = Environment::read(&set).or_detected(&set, &files);
    assert_eq!(env.proxy.unwrap().origin, Origin::Variable("ALL_PROXY".into()));

    let bad = |path: &Path| (path == Path::new("/etc/pip.conf")).then(|| "proxy = http+exec:nc gw\n".to_string());
    let env = Environment::read(&home).or_detected(&home, &bad);
    assert_eq!(env.proxy, None);
    assert_eq!(env.skipped[0].origin, Origin::File(PathBuf::from("/etc/pip.conf")));
    assert_eq!(env.decide("relay.example", Some(443), &[]).1, "none");
}
