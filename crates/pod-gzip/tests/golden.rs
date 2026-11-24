//! The golden vector: the SHA-256 of the members of one input at the fast
//! level. A change of the backend (`flate2`, `miniz_oxide`) that changes a
//! byte fails this test; the commit of the change writes the new value
//! here, as for the wire vectors.

use std::io::Write;

use pod_gzip::{BLOCK, Members};
use sha2::{Digest, Sha256};

/// The input: three blocks and a part, of a text that gzip shrinks and
/// of a xorshift stream that it does not.
fn input() -> Vec<u8> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..3 * BLOCK + 12345)
        .map(|i| {
            if (i / 65536) % 2 == 0 {
                b"the quick brown fox jumps over the lazy dog\n"[i % 44]
            } else {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            }
        })
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The SHA-256 of the output at flate2 1.1.11 and miniz_oxide 0.9.1
/// (`Cargo.lock`), at level 1 and level 6; the same for 1 and 8 threads.
#[test]
fn the_sha256_of_the_members_is_the_golden_vector() {
    let input = input();
    for (level, want) in [
        (1, "a735f6bd6d6ac30408360ad5269955db7aadbfe396083fe54d26a6311aab0046"),
        (6, "929099a82fa2032c76294be86155b5f1ff84db20a11a975992dee65a691572b2"),
    ] {
        for threads in [1, 8] {
            let mut w = Members::with_level(Vec::new(), threads, level);
            w.write_all(&input).unwrap();
            let out = w.finish().unwrap();
            assert_eq!(hex(&Sha256::digest(&out)), want, "level {level}, {threads} threads");
        }
    }
}
