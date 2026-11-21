//! The helpers of the tests of the ARQ as a state machine: two ends with
//! the clock of the test, the datagrams of one end given to the other, and
//! bytes that show a reorder.

use std::time::Instant;

use pod_link::arq::wire::Datagram;
use pod_link::arq::{Arq, ArqSettings};

const MTU: usize = 1200;

pub fn arq(settings: ArqSettings) -> Arq {
    Arq::new(settings, MTU).unwrap()
}

/// Each datagram that `from` gives now.
pub fn drain(from: &mut Arq, now: Instant) -> Vec<Datagram> {
    std::iter::from_fn(|| from.poll_transmit(now)).map(|d| Datagram::decode(&d).unwrap()).collect()
}

pub fn give(to: &mut Arq, now: Instant, datagrams: &[Datagram]) {
    for d in datagrams {
        to.on_datagram(now, &d.to_vec());
    }
}

/// Bytes that differ at each place, so that a reorder shows.
pub fn pattern(n: usize, salt: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt).wrapping_add((i >> 8) as u8)).collect()
}
