//! The fuzz targets of the relay client (`pod-relay`; podssh `relay_list`
//! and `relay_pool`), and of the bearer of a grant (T-1369). The bytes come
//! from the user (a list of relay hosts), from a relay host (its
//! `/relays.json`) or from the peer of a channel (a bearer in the
//! operator's proof).

use pod_e2e::Binding;
use pod_relay::grant::bearer::{self, Bearer, Bearers, Refused};
use pod_relay::pool::parse_pool;
use pod_relay::relay::{forward_path, parse_relay_list};

use crate::{is_one_line, text};

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

/// The handshake hash of the channel of the target's verifier: the one of
/// the first vector `bearer` of `grants.json`, so that its seed holds.
const HANDSHAKE: [u8; 32] = {
    let mut hash = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        hash[i] = i as u8;
        i += 1;
    }
    hash
};

/// A bearer from the peer (`grant::bearer::Bearer::read` and `is_bearer`;
/// new in `pod-relay`, T-1369): a bearer that reads has the size and the
/// magic of the form, and its fields write back to the input. A verifier
/// that takes each signer, on the channel of [`HANDSHAKE`], at the bearer's
/// `iat`, gives one class with its name on one line, and a bearer that it
/// takes is refused as `replayed` the second time.
pub fn relay_bearer(data: &[u8]) {
    let read = Bearer::read(data);
    assert_eq!(read.is_ok(), data.len() == bearer::LEN && bearer::is_bearer(data));
    let Ok(read) = read else {
        assert_eq!(read.err(), Some(Refused::Form));
        return;
    };
    assert_eq!(read.to_bytes()[..], data[..]);
    let bearers = Bearers::default();
    let binding = Binding::of_hash(&HANDSHAKE);
    let verify = || bearers.verify(data, Some(&binding), &read.holder, |_| true, read.iat);
    match verify() {
        Ok(verified) => {
            assert_eq!((verified.signer, verified.iat, verified.exp), (read.signer, read.iat, read.exp));
            assert!(is_one_line(&verified.to_string()));
            assert_eq!(verify(), Err(Refused::Replayed));
        }
        Err(why) => {
            let line = why.to_string();
            assert!(is_one_line(&line) && line.contains(why.name()), "{line}");
        }
    }
}
