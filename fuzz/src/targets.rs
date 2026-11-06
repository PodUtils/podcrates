//! The body of each fuzz target. Each function runs one parser of a crate
//! on untrusted bytes and checks what the parser promises. The doc of each
//! function names its crate and its source.

use pod_proxy::env::{no_proxy_entry_at, no_proxy_matches};
use pod_proxy::http::{decode_chunked, parse_head, parse_status_line};
use pod_proxy::{Carrier, NoProxy, Proxy, authority, digest, proxy_auth};

use crate::{is_one_line, text};

/// Text from a peer made safe for one line (`pod_core::text::one_line`;
/// podssh `one_line`): no control or bidirectional formatting character
/// is left, whitespace is single spaces between words, and a second pass
/// changes nothing.
pub fn one_line(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let safe = pod_core::text::one_line(input);
    assert!(is_one_line(&safe), "{safe:?}");
    assert_eq!(pod_core::text::one_line(&safe), safe);
}

/// A bundle of PEM certificates (`pod_tls::bundle::pem_certificates`;
/// podssh `pem_bundle`), as a CA file or `SSL_CERT_FILE` names it: any
/// bytes, no panic, and no certificate made of nothing.
pub fn pem_bundle(data: &[u8]) {
    if let Ok(certificates) = pod_tls::bundle::pem_certificates(data) {
        assert!(certificates.iter().all(|c| !c.is_empty()));
    }
}

/// An HTTP response head (`pod_proxy::http::parse_head`; podssh
/// `http_head`), as a relay or a proxy sends it: the reason is safe on one
/// line, and each header name is in lower case and each value trimmed, as
/// the readers of the head expect.
pub fn http_head(data: &[u8]) {
    let Some(head) = text(data) else { return };
    let Ok((_, reason, headers)) = parse_head(head) else { return };
    assert!(is_one_line(&reason), "{reason:?}");
    for (name, value) in headers {
        assert_eq!(name, name.trim().to_ascii_lowercase(), "{name:?}");
        assert_eq!(value, value.trim(), "{value:?}");
    }
}

/// A chunked body (`pod_proxy::http::decode_chunked`; podssh
/// `http_chunked`), as a server sends it to the GET client: a body is never
/// longer than its limit, under a small limit and under the largest.
pub fn http_chunked(data: &[u8]) {
    for max in [0, 16, 4096, usize::MAX] {
        if let Ok(Some(body)) = decode_chunked(data, max) {
            assert!(body.len() <= max, "{} bytes past the limit of {max}", body.len());
            assert!(body.len() <= data.len(), "{} bytes from {}", body.len(), data.len());
        }
    }
}

/// The answers of a SOCKS5 proxy (`pod_proxy::fuzz_exchange`, the feature
/// `fuzzing`; podssh `socks5_answer`): the first byte chooses whether the
/// URL has credentials and whether the target goes by name; the rest is
/// what the proxy says before it ends. The exchange ends at once, as the
/// proxy is always ready, and never panics.
pub fn socks5_answer(data: &[u8]) {
    let Some((&choice, answer)) = data.split_first() else { return };
    let _ = pod_proxy::fuzz_exchange(answer, choice & 1 != 0, choice & 2 != 0);
}

/// A proxy URL (`pod_proxy::Proxy::parse`; podssh `proxy_url` and podnet
/// `proxy_url`), as a proxy variable holds it. podssh: a proxy on TCP
/// shows only its host and port, and its URL, which carries the
/// credentials again for a library, parses back to the same proxy. podnet:
/// the label of a line has no user and no password. A proxy on a Unix
/// socket has no credentials, and a variable never names a command.
pub fn proxy_url(data: &[u8]) {
    let Some(url) = text(data) else { return };
    let Ok(proxy) = Proxy::parse(url) else { return };
    match proxy.carrier() {
        Carrier::Tcp => {
            assert_eq!(proxy.to_string(), authority(&proxy.host, proxy.port));
            assert!(!proxy.label().contains('@'), "{}", proxy.label());
        }
        Carrier::Unix(path) => {
            assert!(!proxy.has_credentials());
            assert_eq!(proxy.to_string(), format!("unix:{}", path.display()));
        }
        Carrier::Exec(_) => panic!("Proxy::parse took a command from {url:?}"),
    }
    assert_eq!(Proxy::parse(&proxy.url()), Ok(proxy.clone()), "{}", proxy.url());
}

/// A `NO_PROXY` list against a host (`pod_proxy::env::no_proxy_matches`
/// and `NoProxy`; podssh `no_proxy` and podnet `no_proxy`). podssh: the
/// first line is the list, the rest the host, and `*` matches each host.
/// podnet: the list with ports, against the hosts of the API, of the edge
/// and of the loopback. An entry that excludes a port also excludes the
/// host with no port, and it is an entry of the list.
pub fn no_proxy(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let (list, host) = input.split_once('\n').unwrap_or((input, ""));
    let _ = no_proxy_matches(list, host);
    assert!(no_proxy_matches("*", host), "{host:?}");
    let rules = NoProxy::parse(list);
    for (host, port) in [(host, 443), ("api.trycloudflare.com", 443), ("198.41.192.167", 7844), ("::1", 80)] {
        let entry = no_proxy_entry_at(list, host, Some(port));
        assert_eq!(rules.matches(host, port), entry.is_some(), "{host:?} {port}");
        if let Some(entry) = entry {
            assert!(list.contains(entry), "{entry:?}");
            assert!(no_proxy_matches(list, host), "{entry:?} with no port, {host:?}");
        }
        assert!(NoProxy::parse("*").matches(host, port), "{host:?}");
    }
}

/// A proxy's answer to `CONNECT` (`pod_proxy::http::parse_status_line`;
/// podssh `connect_status` and podnet `connect_response_head`), as the dial
/// reads it: any bytes to the end of the head, as lossy UTF-8. podssh: the
/// reason goes to the user's terminal, so it is safe on one line. The
/// readers of a 407 (`digest::challenge_in`, `proxy_auth::schemes`) take
/// the same head with no panic, and give 8 schemes at most.
pub fn connect_status(data: &[u8]) {
    let head = String::from_utf8_lossy(data);
    if let Ok((_, reason)) = parse_status_line(&head) {
        assert!(is_one_line(&reason), "{reason:?}");
    }
    let _ = digest::challenge_in(&head);
    let schemes = proxy_auth::schemes(&head);
    assert!(schemes.len() <= 8, "{schemes:?}");
}

/// An answer of DNS over HTTPS in RFC 8484's wire form
/// (`pod_dns::wire`, `pod_dns::https`; podssh `dns_wire`), as a resolver
/// sends it: each decoder ends on any bytes, an address has the family of
/// the type asked, an SRV target is a name in lower case that a resolver
/// can be asked for, NXDOMAIN is said for RCODE 3 of a response and for
/// nothing else, and a name read from any offset ends within the bytes, at
/// most 253 characters long. An HTTPS record's list for Encrypted Client
/// Hello is whole by its own lengths, within its bound, from an answer or
/// from a record's data alone.
pub fn dns_wire(data: &[u8]) {
    use pod_dns::{NXDOMAIN, https, wire};
    const NAME: &str = "tcp.ssh.relay.ajam.dev";
    const SRV_NAME: &str = "_v2-origintunneld._tcp.argotunnel.com";
    let rcode3 = data.len() >= 12 && (data[2] & 0x80) != 0 && (data[3] & 0x0f) == 3;
    for (kind, v6) in [(wire::A, false), (wire::AAAA, true)] {
        match wire::addrs(data, NAME, kind) {
            Ok(ips) => assert!(ips.iter().all(|ip| ip.is_ipv6() == v6), "{ips:?}"),
            Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
        }
    }
    match wire::srv(data, SRV_NAME) {
        Ok(records) => {
            for r in records {
                let safe =
                    r.target.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b));
                assert!(safe && !r.target.is_empty() && r.target.len() <= 253, "{r:?}");
            }
        }
        Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
    }
    let whole = |list: &[u8]| https::check_list(list).is_ok() && list.len() <= https::MAX_LIST;
    match https::from_wire(data, NAME) {
        Ok(Some(found)) => assert!(whole(&found.list), "{found:?}"),
        Ok(None) => {}
        Err(e) => assert_eq!(e == NXDOMAIN, rcode3, "{e}"),
    }
    if let Ok(Some((_, list))) = https::from_rdata(data) {
        assert!(whole(&list), "{list:?}");
    }
    let start = data.first().map_or(0, |&b| usize::from(b) % data.len().max(1));
    if let Ok((name, end)) = wire::read_name(data, start) {
        assert!(end <= data.len() && name.len() <= 253, "{name:?} ends at {end}");
    }
}

/// An answer of DNS over HTTPS in the JSON form (`pod_dns::json`,
/// `pod_dns::https::from_json`; podnet `doh_json`), as a resolver sends it:
/// podnet's reading of each record's data as an address and as an SRV
/// record (64 records at most), with no panic; the status of an answer
/// decides NXDOMAIN for each type; an SRV target is a name in lower case
/// that a resolver can be asked for; an ECH list is whole by its own
/// lengths, within its bound.
pub fn doh_json(data: &[u8]) {
    use pod_dns::{NXDOMAIN, https, json};
    if let Ok(answer) = json::parse(data) {
        for record in answer.records.iter().take(64) {
            let _ = record.data.parse::<std::net::IpAddr>();
            if let Some(srv) = json::srv_of(&record.data) {
                assert!(is_safe_name(&srv.target), "{srv:?}");
            }
        }
        for want in [1, 28, 33, 65] {
            let said = answer.of(want).err();
            assert_eq!(said.as_deref() == Some(NXDOMAIN), answer.status == Some(3), "{said:?}");
        }
    }
    if let Ok(records) = json::parse_srv(data) {
        assert!(records.iter().all(|r| is_safe_name(&r.target)), "{records:?}");
    }
    if let Ok(Some(found)) = https::from_json(data) {
        assert!(https::check_list(&found.list).is_ok() && found.list.len() <= https::MAX_LIST, "{found:?}");
    }
}

/// A name in lower case that a resolver can be asked for: letters, digits,
/// `-`, `.` and `_`, 1 to 253 bytes.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
}
