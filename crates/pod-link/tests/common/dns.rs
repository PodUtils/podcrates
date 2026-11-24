//! The helpers of the tests of the DNS carrier: a carrier of a made-up
//! identity under a test zone, a listener on the loopback, a dial address,
//! and ARQ and DNS settings that end a lost link fast.

use std::time::Duration;

use pod_identity::{KeyName, PublicKey};
use pod_link::arq::ArqSettings;
use pod_link::dns::{Dns, DnsListener, DnsSettings};
use pod_link::tcp::Keys;

use super::tcp::{id, ring};

/// The zone of the tests.
pub const ZONE: &str = "l.test";

/// DNS settings fast enough for the loopback: many queries a second and a
/// short idle poll, so a handshake and a transfer do not wait.
pub fn fast_dns() -> DnsSettings {
    DnsSettings {
        queries_per_second: 20000.0,
        in_flight: 64,
        poll_min: Duration::from_millis(2),
        ..DnsSettings::default()
    }
}

/// An ARQ that gives up a few retries, so a silent peer ends a lost link in
/// about one second, not a minute.
pub fn fast_arq() -> ArqSettings {
    ArqSettings {
        retries: 3,
        rto_initial: Duration::from_millis(100),
        rto_floor: Duration::from_millis(50),
        rto_ceiling: Duration::from_millis(300),
        ..ArqSettings::default()
    }
}

/// A carrier of the identity `n` on ring, under [`ZONE`], whose listeners
/// take the keys of `clients`.
pub fn carrier(n: u8, clients: &[PublicKey]) -> Dns {
    let mut dns = Dns::new(ring(), id(n)).with_clients(Keys::new(clients.iter().map(|k| KeyName::Key(*k)))).zone(ZONE);
    dns.dns = fast_dns();
    dns
}

/// A listener of `dns` on a free UDP port of the loopback.
pub async fn bound(dns: &Dns) -> DnsListener {
    dns.bind("127.0.0.1:0".parse().unwrap()).await.expect("bind on the loopback")
}

/// The dial address of a listener whose key is `key`: `KEY@ZONE`.
pub fn at(key: &PublicKey) -> String {
    format!("{}@{}", key.hex(), ZONE)
}

/// The query name of a dialer for the datagram `payload` of the session
/// `session` at `counter`, under [`ZONE`]: base32 of RFC 4648 (lower case,
/// no padding) in labels of at most 63 characters. A test sends a query of
/// its own session with it.
pub fn query_name(session: u32, counter: u32, payload: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let blob = [&session.to_be_bytes()[..], &counter.to_be_bytes(), payload].concat();
    let (mut chars, mut word, mut bits) = (Vec::new(), 0u32, 0u32);
    for byte in blob {
        word = (word << 8 | u32::from(byte)) & 0xfff;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            chars.push(ALPHABET[(word >> bits & 31) as usize]);
        }
    }
    if bits > 0 {
        chars.push(ALPHABET[(word << (5 - bits) & 31) as usize]);
    }
    let labels: Vec<&str> = chars.chunks(63).map(|l| std::str::from_utf8(l).expect("base32 is ASCII")).collect();
    format!("{}.{ZONE}", labels.join("."))
}
