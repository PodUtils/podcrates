//! The tests of the consumers that test these values, not code, with their
//! names: podssh `crates/podssh-relay/src/relay.rs`, `pool.rs`,
//! `quick/edge.rs` and `crates/podssh-cli/src/doctor/relay_checks.rs`;
//! podnet `src/trust.rs`, `src/cloudflare/mod.rs` and `src/resolve.rs`.

use std::net::IpAddr;

use pod_services::Service;

fn service(id: &str) -> &'static Service {
    pod_services::get(id).unwrap_or_else(|| panic!("no service {id}"))
}

/// podssh `relay.rs` (T-342): the limits that podssh compiled in
/// (`RELAY_IDLE_SECS` 180, `SESSION_TIME_CAP` 12 h, `SESSION_BYTE_CAP`
/// 64 MiB, `FORWARD_MAX_FRAME` 256 KiB) are the facts of the relay's
/// limits, which the watch compares with the live `/relays.json`; a change
/// of the facts fails here until the consumers' budgets move with them. The
/// facts are read off the text of the file, as podssh read
/// `relay-limits.toml`, and through the API.
#[test]
fn the_compiled_limits_are_the_facts_of_the_relays_limits() {
    let facts: toml::Table = include_str!("../services/ajam-relay.toml").parse().unwrap();
    let value = |name: &str| -> u64 {
        let fact = facts["fact"].as_array().unwrap().iter().find(|f| f["name"].as_str() == Some(name));
        let text = fact.unwrap_or_else(|| panic!("ajam-relay.toml holds no {name}"))["value"].as_integer().unwrap();
        assert_eq!(service("ajam-relay").int(name), Some(text), "{name}: the API gives the file's value");
        text as u64
    };
    assert_eq!(value("idle-timeout-ms"), 180 * 1000);
    assert_eq!(value("max-session-ms"), 12 * 3600 * 1000);
    assert_eq!(value("max-session-bytes"), 64 << 20);
    assert_eq!(value("max-frame-bytes"), 262_144);
    assert_eq!(value("connect-timeout-ms"), 10_000);
}

/// podssh `pool.rs`: each seed host is of the default host's deployment,
/// a host under the parent domain of the default host, to which a token
/// may go (pod-relay keeps the rule, `same_deployment`, and its tests).
#[test]
fn the_seed_hosts_belong_to_the_default_deployment() {
    let relay = service("ajam-relay");
    let default = relay.endpoint("default").unwrap().host();
    let parent = default.split_once('.').map(|(_, rest)| rest).unwrap();
    assert_eq!(parent, "ssh.relay.ajam.dev");
    let seed: Vec<&str> = relay.endpoints_of("seed").map(|e| e.host()).collect();
    assert_eq!(seed.len(), 6);
    for host in seed {
        let (label, rest) = host.split_once('.').unwrap();
        assert!(rest == parent && !label.is_empty() && host != default, "{host}");
    }
}

/// podssh `doctor/relay_checks.rs`: each of GitHub's keys is an OpenSSH
/// SHA-256 fingerprint.
#[test]
fn the_published_keys_are_well_formed_sha256_fingerprints() {
    for key in service("github-ssh").texts("host-key-sha256").unwrap() {
        let b64 = key.strip_prefix("SHA256:").expect(key);
        assert_eq!(b64.len(), 43, "{key}");
        assert!(b64.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'), "{key}");
    }
}

fn roots() -> &'static str {
    service("cloudflare-tunnel-edge").text("roots").expect("the edge roots")
}

fn first_end() -> String {
    service("cloudflare-tunnel-edge").date("roots-first-end").expect("the first end").to_string()
}

/// podssh `quick/edge.rs`: the ends of cloudflared's roots, as OpenSSL
/// reads them, and the first of them is the day that doctor names.
#[test]
fn the_first_end_of_cloudflared_s_roots_is_the_day_that_doctor_names() {
    let ends = pod_tls::not_after_dates(roots().as_bytes()).unwrap();
    // The file's comments, read off each certificate by OpenSSL.
    assert_eq!(ends, ["2029-08-15", "2029-08-15", "2029-11-01"]);
    assert_eq!(ends.iter().min().cloned(), Some(first_end()));
}

/// podnet `trust.rs`: the fact is the first notAfter of the edge roots,
/// and it is in the future.
#[test]
fn the_first_end_of_the_edge_roots() {
    let mut dates = pod_tls::not_after_dates(roots().as_bytes()).unwrap();
    assert!(dates.len() >= 3, "{dates:?}");
    dates.sort();
    assert_eq!(dates[0], first_end(), "{dates:?}");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let (year, _, _) = pod_core::clock::civil_from_days(now / 86_400);
    let end: i64 = first_end()[..4].parse().unwrap();
    assert!(end >= year, "an edge root ends on {}", first_end());
}

/// podnet `cloudflare/mod.rs`: if the roots were emptied or cut, TLS to the
/// edge would fail with UnknownIssuer, so the roots must parse.
#[test]
fn the_embedded_edge_roots_are_real_certificates() {
    assert!(roots().len() > 1000, "the embedded roots look empty: {} bytes", roots().len());
    let certs = pod_tls::bundle::pem_certificates(roots().as_bytes()).unwrap();
    assert!(certs.len() >= 3, "expected Cloudflare's edge roots, found {}", certs.len());
}

/// podnet `cloudflare/mod.rs`: the ALPN and the SNI of QUIC are two words.
/// The ALPN given as the SNI completes the key derivation and then dies in
/// silence, after a handshake of 10 s.
#[test]
fn quic_alpn_and_sni_are_different_strings() {
    let edge = service("cloudflare-tunnel-edge");
    for quic in edge.endpoints_of("quic") {
        let alpn: Vec<&str> = quic.alpn().iter().map(|a| a.as_ref()).collect();
        assert_eq!(alpn, ["argotunnel"]);
        assert_eq!(quic.server_name(), "quic.cftunnel.com");
        assert_ne!(alpn[0], quic.server_name(), "SNI must not be the ALPN value");
    }
    assert_eq!(edge.endpoints_of("quic").count(), 2);
    for h2 in edge.endpoints_of("h2") {
        assert_eq!(h2.server_name(), "h2.cftunnel.com");
        assert!(h2.alpn().is_empty(), "HTTP/2 to the edge offers no ALPN");
    }
}

/// podnet `resolve.rs`: the DoH servers are IP addresses of two operators
/// or more, and Cloudflare and Google each have IPv4 and IPv6 servers (an
/// IPv6-only host).
#[test]
fn the_servers_are_ip_addresses_of_two_operators() {
    let doh: Vec<&Service> = pod_services::of_kind("doh").collect();
    let mut operators: Vec<&str> = doh.iter().map(|s| s.party()).collect();
    operators.dedup();
    assert!(operators.len() >= 2);
    for server in &doh {
        for endpoint in server.endpoints() {
            assert!(!endpoint.addresses().is_empty(), "{} {}", server.id(), endpoint.name());
            for address in endpoint.addresses() {
                assert!(address.parse::<IpAddr>().is_ok(), "{address}");
            }
        }
    }
    for id in ["cloudflare-doh", "google-doh"] {
        let of =
            |v6: bool| service(id).endpoints().iter().flat_map(|e| e.ip_addrs()).filter(|a| a.is_ipv6() == v6).count();
        assert!(of(true) >= 1 && of(false) >= 1, "{id}");
    }
}
