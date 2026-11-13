//! The seeds of the fuzz target `e2e_record`: records that the two ends of
//! the exporter of fixed keys seal (`pod_e2e::fuzzing::records`). Each seed
//! is a byte of flags and a stream of records, as the target reads them.
//! The seeds on disk are the ones that this file makes, and each record of
//! them opens but for the faults that the seed is made for, so that the
//! fuzzer starts past the check of each record. Write them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_e2e_record_seeds -- --ignored`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pod_core::entropy::{Entropy, NoRandom};
use pod_e2e::fuzzing::records;
use pod_e2e::record::{Records, Settings, Suite, record_len};

/// A type of the caller's records.
const DATA: u8 = 1;

/// The flags of a seed: AES-256-GCM, 660 s between two records, one failed
/// RESUME as the limit, the initiator opens.
const AES: u8 = 1;
const LATE: u8 = 3 << 1;
const ONE_FAILURE: u8 = 8;
const AT_THE_INITIATOR: u8 = 16;

/// Known bytes for the fresh prefix of a RESUME.
struct Counting(u8);

impl Entropy for Counting {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        for b in buf {
            self.0 = self.0.wrapping_add(1);
            *b = self.0;
        }
        Ok(())
    }
}

/// The end of the initiator, which seals with `settings`.
fn sealer(suite: Suite, settings: Settings, now: Instant) -> Records {
    records(true, suite, settings, now).expect("the records of the initiator")
}

/// `n` records of `end`, each of its own payload.
fn sealed(end: &mut Records, now: Instant, n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|i| end.seal(now, DATA, 0, i as u32, format!("record {i}").as_bytes()).expect("a seal")).collect()
}

/// A seed: the flags, then the records in their order.
fn seed(flags: u8, parts: &[&Vec<u8>]) -> Vec<u8> {
    let mut out = vec![flags];
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

/// Each seed: its name, its bytes, and the count of its records that fail.
fn seeds() -> Vec<(&'static str, Vec<u8>, usize)> {
    let now = Instant::now();
    let chacha = Suite::ChaCha20Poly1305;
    let mut out = Vec::new();

    let mut a = sealer(chacha, Settings::DEFAULT, now);
    let kinds = [
        a.seal(now, DATA, 0, 1, b"first").unwrap(),
        a.seal(now, 2, 0x8000, 7, b"").unwrap(),
        a.seal(now, DATA, 0x0002, 0, b"the third record").unwrap(),
    ];
    out.push(("chacha", seed(0, &kinds.iter().collect::<Vec<_>>()), 0));

    let mut a = sealer(Suite::Aes256Gcm, Settings { pad: true, ..Settings::DEFAULT }, now);
    let padded: Vec<Vec<u8>> =
        [0usize, 1, 129, 3000].iter().map(|&n| a.seal(now, DATA, 0, 3, &vec![0x5a; n]).unwrap()).collect();
    out.push(("aes_padded", seed(AES, &padded.iter().collect::<Vec<_>>()), 0));

    // A key of two records: the epochs 0, 0, 1, 1, 2; a late record of the
    // epoch before opens with the key before.
    let mut a = sealer(chacha, Settings { rekey_records: 2, ..Settings::DEFAULT }, now);
    let r = sealed(&mut a, now, 5);
    out.push(("rekey", seed(0, &[&r[0], &r[2], &r[1], &r[3], &r[4]]), 0));

    let mut a = sealer(chacha, Settings::DEFAULT, now);
    let r = sealed(&mut a, now, 2);
    out.push(("replay", seed(0, &[&r[0], &r[1], &r[1], &r[0]]), 2));

    // The record 1 went with the lost carrier; the record 2 takes the fresh
    // prefix of the RESUME.
    let mut a = sealer(chacha, Settings::DEFAULT, now);
    let r = sealed(&mut a, now, 2);
    let resume = a.resume(&mut Counting(0)).unwrap();
    let after = a.seal(now, DATA, 0, 0, b"after the resume").unwrap();
    out.push(("resume", seed(0, &[&r[0], &resume, &after]), 0));
    out.push(("resume_late", seed(LATE, &[&r[0], &resume, &after]), 2));
    out.push(("resume_twice", seed(ONE_FAILURE, &[&r[0], &resume, &resume, &after]), 2));

    let mut b = records(false, Suite::Aes256Gcm, Settings::DEFAULT, now).unwrap();
    let r = sealed(&mut b, now, 2);
    out.push(("from_the_responder", seed(AES | AT_THE_INITIATOR, &[&r[0], &r[1]]), 0));
    out
}

/// The records of `seed` that fail at the end that its flags pick, opened
/// as the target opens them.
fn failures(seed: &[u8]) -> usize {
    let (flags, mut rest) = (seed[0], &seed[1..]);
    let suite = if flags & AES == 0 { Suite::ChaCha20Poly1305 } else { Suite::Aes256Gcm };
    let step = Duration::from_secs([0, 1, 300, 660][usize::from((flags >> 1) & 3)]);
    let settings = Settings { resume_failures: if flags & ONE_FAILURE == 0 { 3 } else { 1 }, ..Settings::DEFAULT };
    let mut now = Instant::now();
    let mut end = records(flags & AT_THE_INITIATOR != 0, suite, settings, now).unwrap();
    let mut failed = 0;
    while !rest.is_empty() {
        let len = record_len(rest).expect("a header of a seed reads");
        let (record, after) = rest.split_at(len);
        rest = after;
        now += step;
        failed += usize::from(end.open(now, record).is_err());
    }
    failed
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join("e2e_record")
}

/// The seeds on disk are the ones of [`seeds`], and each fails only where
/// it is made to.
#[test]
fn the_seeds_of_e2e_record_are_sealed_records_that_open() {
    for (name, bytes, failing) in seeds() {
        let on_disk = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(on_disk == bytes, "{name} is stale: run write_the_e2e_record_seeds");
        assert_eq!(failures(&bytes), failing, "{name}");
        crate::targets::e2e_record(&bytes);
    }
}

#[test]
#[ignore = "writes the seeds of e2e_record into fuzz/seeds/e2e_record"]
fn write_the_e2e_record_seeds() {
    std::fs::create_dir_all(dir()).unwrap();
    for (name, bytes, _) in seeds() {
        std::fs::write(dir().join(name), bytes).unwrap();
    }
}
