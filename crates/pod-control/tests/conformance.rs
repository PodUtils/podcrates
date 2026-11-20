//! The conformance check against the server of this crate, at each tier,
//! and against a server that breaks the protocol.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

use pod_control::{Client, ClientSettings, Tier, conformance};

fn check(path: &std::path::Path) -> Vec<conformance::Finding> {
    let mut connect = || Client::connect_unix(path, ClientSettings::default());
    conformance::check(&mut connect, &conformance::Options::default())
}

#[test]
fn the_server_of_the_crate_conforms_at_each_tier() {
    for tier in Tier::ALL {
        let mut settings = common::settings();
        settings.same_user = tier;
        let (_server, path, log) = common::start(&format!("conform-{}", tier.name()), settings);
        let found = check(&path);
        assert!(found.is_empty(), "{tier}: {found:#?}");
        // The check changes nothing: only status and metrics reach the host.
        assert!(log.lock().unwrap().iter().all(|l| l.starts_with("status") || l == "metrics"));
    }
}

/// A server that answers each line with one fixed line: the check finds
/// its faults.
#[test]
fn a_server_that_breaks_the_protocol_has_findings() {
    let path = common::dir("conform-broken").join("ctl.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            std::thread::spawn(move || {
                let mut w = &stream;
                for line in BufReader::new(&stream).split(b'\n') {
                    if line.is_err() || w.write_all(b"{\"schema\":1,\"id\":1,\"ok\":true,\"result\":{}}\n").is_err() {
                        return;
                    }
                }
            });
        }
    });
    let found = check(&path);
    let checks: Vec<&str> = found.iter().map(|f| f.check).collect();
    for name in ["version", "unknown-method", "not-json", "too-long", "methods"] {
        assert!(checks.contains(&name), "no finding of {name}: {found:#?}");
    }
}
