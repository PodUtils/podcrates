//! The fuzz targets of the relay client (`pod-relay`; podssh `relay_list`
//! and `relay_pool`). The bytes come from the user (a list of relay hosts)
//! or from a relay host (its `/relays.json`).

use pod_relay::pool::parse_pool;
use pod_relay::relay::{forward_path, parse_relay_list};

use crate::text;

/// The primary host of the pool's checks: the default deployment's.
const PRIMARY: &str = "tcp.ssh.relay.ajam.dev";

/// The form of a forward path for `host` and `port`: always
/// `/connect/HOST/PORT`, three segments, with nothing that could add a
/// segment, a query or a line to the request.
fn check(host: &str, port: u16) {
    let Ok(path) = forward_path(host, port) else { return };
    assert_eq!(path, format!("/connect/{host}/{port}"));
    assert_eq!(path.split('/').count(), 4, "{path:?}");
    assert!(
        !host.chars().any(|c| matches!(c, '?' | '#' | '%' | '@') || c.is_whitespace() || c.is_control()),
        "{host:?}"
    );
}

/// A list of relay hosts (`pod_relay::relay::parse_relay_list`), as a flag
/// or a variable of the consumer gives it, and the path of a forward
/// session (`forward_path`) for each host and for the input itself.
pub fn relay_list(data: &[u8]) {
    let Some(input) = text(data) else { return };
    if let Ok(relays) = parse_relay_list(input) {
        for relay in relays {
            check(&relay.host, relay.port);
        }
    }
    let port = input.len() as u16;
    check(input, port);
}

/// A `/relays.json` body (`pod_relay::pool::parse_pool`), as the relay
/// sends it: each host is kept once, in lower case, and belongs to the
/// deployment of the primary host and is not the primary itself.
pub fn relay_pool(data: &[u8]) {
    let Ok(hosts) = parse_pool(data, PRIMARY) else { return };
    for (i, host) in hosts.iter().enumerate() {
        assert_eq!(host, &host.to_ascii_lowercase());
        assert!(host.ends_with(".ssh.relay.ajam.dev") && host != PRIMARY, "{host:?}");
        assert!(!hosts[..i].contains(host), "{host:?} twice");
    }
}
