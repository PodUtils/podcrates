//! The seeds of the fuzz target `link_kcp`: the datagrams of a peer of the
//! conversation 7, in the form of the target (a byte of setup, then for each
//! datagram a step of the clock, its length and its bytes). The seeds on
//! disk are the ones that this file makes. Write them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_link_kcp_seeds -- --ignored`.

use std::path::PathBuf;

/// The commands of KCP.
const PUSH: u8 = 81;
const ACK: u8 = 82;
const WASK: u8 = 83;
const WINS: u8 = 84;

/// One segment of KCP of the conversation 7, with a window of 256.
fn segment(cmd: u8, frg: u8, ts: u32, sn: u32, una: u32, data: &[u8]) -> Vec<u8> {
    let mut out = 7u32.to_le_bytes().to_vec();
    out.extend_from_slice(&[cmd, frg]);
    out.extend_from_slice(&256u16.to_le_bytes());
    for value in [ts, sn, una, u32::try_from(data.len()).expect("a segment fits u32")] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(data);
    out
}

/// A seed of `setup` with each datagram one step (10 ms) after the one
/// before it.
fn seed(setup: u8, datagrams: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![setup];
    for datagram in datagrams {
        out.push(1);
        out.extend_from_slice(&u16::try_from(datagram.len()).expect("a datagram fits u16").to_be_bytes());
        out.extend_from_slice(datagram);
    }
    out
}

/// The name and the bytes of each seed.
fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    // A peer that sends 3 segments of data, acknowledges the 2 segments of
    // the stream (128 bytes of setup), and asks for and tells its window.
    let mut transfer = Vec::new();
    for sn in 0..3u32 {
        transfer.push(segment(PUSH, 0, 10 * sn, sn, 0, &[0xa5; 1000]));
    }
    let mut acks = segment(ACK, 0, 0, 0, 0, &[]);
    acks.extend_from_slice(&segment(ACK, 0, 0, 1, 2, &[]));
    transfer.push(acks);
    transfer.push(segment(WASK, 0, 40, 3, 2, &[]));
    transfer.push(segment(WINS, 0, 50, 3, 2, &[]));
    // An acknowledgement whose time stamp is 2^31 ms old: the arithmetic of
    // `kcp` overflows (a panic with overflow checks), and the guard of the
    // carrier ends the stream.
    let old_ack = vec![segment(PUSH, 0, 0, 0, 0, b"hello"), segment(ACK, 0, 0x8000_0000, 0, 0, &[])];
    // A fragment of a message (the stream mode sends none), a segment of
    // 2000 bytes, and datagrams of a bad form: too short, an unknown
    // command, a length past the end, another conversation.
    let mut short_len = segment(PUSH, 0, 0, 1, 0, b"ab");
    short_len.truncate(short_len.len() - 1);
    let mut other = segment(PUSH, 0, 0, 1, 0, b"x");
    other[0] = 8;
    let forms = vec![
        segment(PUSH, 255, 0, 0, 0, b"x"),
        segment(PUSH, 0, 0, 0, 0, &[0x3c; 2000]),
        vec![7, 0, 0, 0, PUSH],
        segment(99, 0, 0, 1, 0, &[]),
        short_len,
        other,
    ];
    vec![("transfer", seed(2, &transfer)), ("old-ack", seed(1, &old_ack)), ("bad-forms", seed(0, &forms))]
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join("link_kcp")
}

/// The seeds on disk are the ones of [`seeds`]. The target takes each: a
/// peer that goes silent ends the stream at the silence limit; with
/// overflow checks the old acknowledgement makes `kcp` panic, and the guard
/// of the carrier ends the stream with `BrokenPipe`.
#[test]
fn the_seeds_of_link_kcp_are_the_datagrams_of_a_peer() {
    for (name, bytes) in seeds() {
        let on_disk = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(on_disk == bytes, "{name} is stale: run write_the_link_kcp_seeds");
        let (panics, end) = crate::targets::run_kcp(&bytes).expect("a run");
        let expect = if name == "old-ack" && cfg!(debug_assertions) {
            (1, std::io::ErrorKind::BrokenPipe)
        } else {
            (0, std::io::ErrorKind::TimedOut)
        };
        assert_eq!((panics, end.kind()), expect, "{name}: {end}");
    }
}

#[test]
#[ignore = "writes the seeds of link_kcp into fuzz/seeds/link_kcp"]
fn write_the_link_kcp_seeds() {
    std::fs::create_dir_all(dir()).unwrap();
    for (name, bytes) in seeds() {
        std::fs::write(dir().join(name), bytes).unwrap();
    }
}
