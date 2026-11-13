//! The seeds of the fuzz target `link_arq`: the datagrams that a peer sends
//! in a short exchange with the ARQ of the target, in the form of the target
//! (a byte of setup, then for each datagram a step of the clock, its length
//! and its bytes). The seeds on disk are the ones that this file makes. Write
//! them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_link_arq_seeds -- --ignored`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pod_link::arq::wire::Datagram;
use pod_link::arq::{Arq, ArqSettings};

use crate::targets::ARQ_MTU;

/// One step of the clock of a seed: 10 ms.
const STEP: Duration = Duration::from_millis(10);

fn push(seed: &mut Vec<u8>, datagram: &[u8]) {
    seed.push(1);
    seed.extend_from_slice(&u16::try_from(datagram.len()).expect("a datagram fits u16").to_be_bytes());
    seed.extend_from_slice(datagram);
}

/// The datagrams of a peer that writes `peer_bytes` (and closes when
/// `close`) to an ARQ that writes `setup * 64` bytes, as the target does.
fn exchange(setup: u8, peer_bytes: usize, close: bool) -> Vec<u8> {
    let mut seed = vec![setup];
    let mut now = Instant::now();
    let mut ours = Arq::new(ArqSettings::default(), ARQ_MTU).unwrap();
    let mut peer = Arq::new(ArqSettings::default(), ARQ_MTU).unwrap();
    ours.write(&vec![0x5a; usize::from(setup) * 64]).unwrap();
    peer.write(&vec![0xa5; peer_bytes]).unwrap();
    if close {
        peer.close();
    }
    for _ in 0..64 {
        while let Some(out) = ours.poll_transmit(now) {
            peer.on_datagram(now, &out);
        }
        while let Some(out) = peer.poll_transmit(now) {
            push(&mut seed, &out);
            ours.on_datagram(now, &out);
        }
        now += STEP;
        let mut buf = [0u8; 4096];
        while ours.read(&mut buf).unwrap_or(0) > 0 {}
        while peer.read(&mut buf).unwrap_or(0) > 0 {}
    }
    seed
}

/// The name and the bytes of each seed.
fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut forms = vec![0];
    for datagram in [
        &[][..],
        &[1, 0, 0, 0, 0, 0, 1, 0, 1][..],
        &[1, 0, 0, 0, 0, 0, 2, 0, 1, 9][..],
        &[2, 0, 0][..],
        &[3, 0, 0, 0, 9, 0, 1][..],
        &[4, 4][..],
        &[9][..],
    ] {
        push(&mut forms, datagram);
    }
    let mut probe = vec![2];
    push(&mut probe, &Datagram::Probe.to_vec());
    vec![
        ("transfer", exchange(4, 3000, false)),
        ("fin", exchange(1, 500, true)),
        ("acks-only", exchange(16, 0, false)),
        ("probe", probe),
        ("bad-forms", forms),
    ]
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join("link_arq")
}

/// The seeds on disk are the ones of [`seeds`], and the target takes each.
#[test]
fn the_seeds_of_link_arq_are_the_datagrams_of_an_exchange() {
    for (name, bytes) in seeds() {
        let on_disk = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(on_disk == bytes, "{name} is stale: run write_the_link_arq_seeds");
        crate::targets::link_arq(&bytes);
    }
    let (_, transfer) = &seeds()[0];
    assert!(transfer.len() > 3000, "the transfer seed carries the peer's bytes: {} bytes", transfer.len());
}

#[test]
#[ignore = "writes the seeds of link_arq into fuzz/seeds/link_arq"]
fn write_the_link_arq_seeds() {
    std::fs::create_dir_all(dir()).unwrap();
    for (name, bytes) in seeds() {
        std::fs::write(dir().join(name), bytes).unwrap();
    }
}
