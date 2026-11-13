//! The records between two ends of one exporter: a sealed record opens on
//! the other end, a changed byte fails, a repeat is refused and counted, a
//! rekey keeps the stream, the padding gives the bucket sizes, and a RESUME
//! works once.

use std::time::{Duration, Instant};

use pod_core::entropy::{Entropy, NoRandom};

use super::*;
use crate::exporter::Exporter;

const DATA: u8 = 1;

/// Known bytes for the fresh prefixes.
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

fn ends(suite: Suite, settings: Settings, now: Instant) -> (Records, Records) {
    let keys = [11u8; 64];
    let a = Exporter::new(&[2u8; 32], &keys, true).records(suite, settings, now).unwrap();
    let b = Exporter::new(&[2u8; 32], &keys, false).records(suite, settings, now).unwrap();
    (a, b)
}

fn payload(opened: Result<Opened, Fault>) -> Vec<u8> {
    match opened.unwrap() {
        Opened::Record(r) => r.payload,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_sealed_record_opens_on_the_other_end_in_each_suite() {
    let now = Instant::now();
    for suite in Suite::ALL {
        let (mut a, mut b) = ends(suite, Settings::DEFAULT, now);
        let sealed = a.seal(now, DATA, 0x8000, 7, b"to the node").unwrap();
        // A record of one direction does not open as one of the other.
        assert_eq!(a.open(now, &sealed).unwrap_err(), Fault::Tampered);
        assert_eq!(sealed.len(), HEADER + 11 + crate::TAG);
        assert_eq!(record_len(&sealed[..HEADER]).unwrap(), sealed.len());
        assert!(!sealed.windows(4).any(|w| w == b"node"), "no plain text on the wire");
        let want = Record { kind: DATA, flags: 0x8000, stream: 7, counter: 0, payload: b"to the node".to_vec() };
        assert_eq!(b.open(now, &sealed).unwrap(), Opened::Record(want));
        let back = b.seal(now, DATA, 0, 9, b"to the operator").unwrap();
        assert_eq!(payload(a.open(now, &back)), b"to the operator");
        assert_eq!((a.counters(), b.counters()), ((1, 1), (1, 1)));
    }
    // Two suites share no key.
    let (mut chacha, _) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let (_, mut aes) = ends(Suite::Aes256Gcm, Settings::DEFAULT, now);
    let sealed = chacha.seal(now, DATA, 0, 0, b"x").unwrap();
    assert_eq!(aes.open(now, &sealed).unwrap_err(), Fault::Tampered);
}

#[test]
fn each_changed_byte_fails_and_leaves_the_layer_as_it_was() {
    let now = Instant::now();
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings { pad: true, ..Settings::DEFAULT }, now);
    let sealed = a.seal(now, DATA, 0, 3, b"payload").unwrap();
    for i in 0..sealed.len() {
        let mut changed = sealed.clone();
        changed[i] ^= 0x01;
        assert!(b.open(now, &changed).is_err(), "byte {i}");
    }
    assert!(b.open(now, &sealed[..sealed.len() - 1]).is_err(), "a record cut short");
    assert_eq!(payload(b.open(now, &sealed)), b"payload", "the record itself opens after");
    assert_eq!(b.replayed(), 0, "no failure counts as a repeat");
}

#[test]
fn a_replayed_record_is_refused_and_counted_and_a_late_one_opens() {
    let now = Instant::now();
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let sealed: Vec<_> = (0..4u8).map(|i| a.seal(now, DATA, 0, 0, &[i]).unwrap()).collect();
    assert_eq!(payload(b.open(now, &sealed[0])), [0]);
    assert_eq!(payload(b.open(now, &sealed[2])), [2]);
    assert_eq!(b.open(now, &sealed[0]).unwrap_err(), Fault::Replayed(0));
    assert_eq!(b.open(now, &sealed[2]).unwrap_err(), Fault::Replayed(2));
    assert_eq!(b.replayed(), 2);
    assert_eq!(payload(b.open(now, &sealed[1])), [1], "a late record inside the window");
    assert_eq!(payload(b.open(now, &sealed[3])), [3]);
    // A record at the edge of the window is stale.
    let old = a.seal(now, DATA, 0, 0, b"old").unwrap();
    for _ in 0..WINDOW {
        let r = a.seal(now, DATA, 0, 0, b"").unwrap();
        b.open(now, &r).unwrap();
    }
    assert_eq!(b.open(now, &old).unwrap_err(), Fault::Stale(4));
}

#[test]
fn a_rekey_by_count_by_bytes_and_by_age_keeps_the_stream() {
    let now = Instant::now();
    let tiny = Settings { rekey_records: 3, rekey_bytes: 1 << 40, ..Settings::DEFAULT };
    let (mut a, mut b) = ends(Suite::Aes256Gcm, tiny, now);
    let sealed: Vec<_> = (0..10u8).map(|i| a.seal(now, DATA, 0, 0, &[i]).unwrap()).collect();
    assert_eq!(a.epochs().0, 3, "a new key after each 3 records");
    assert_eq!(sealed[3][3] & 1, 1, "the phase flips with the key");
    // Out of order across the change of a key: the 4th record (the next
    // key) before the 3rd (the key before).
    for i in [0, 1, 3, 2, 4, 5, 6, 7, 8, 9] {
        assert_eq!(payload(b.open(now, &sealed[i])), [i as u8], "record {i}");
    }
    assert_eq!(b.epochs().1, 3);
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings { rekey_bytes: 100, ..Settings::DEFAULT }, now);
    for i in 0..4 {
        let r = a.seal(now, DATA, 0, 0, &[0u8; 60]).unwrap();
        assert_eq!(payload(b.open(now, &r)).len(), 60, "record {i}");
    }
    assert_eq!(a.epochs().0, 1, "a new key after 100 bytes");
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let later = now + Settings::DEFAULT.rekey_age;
    for at in [now, later, later, later + Settings::DEFAULT.rekey_age] {
        let r = a.seal(at, DATA, 0, 0, b"tick").unwrap();
        assert_eq!(payload(b.open(at, &r)), b"tick");
    }
    assert_eq!((a.epochs().0, b.epochs().1), (2, 2), "a new key after each hour");
}

#[test]
fn the_padding_gives_the_bucket_sizes_and_goes_before_the_payload_comes_out() {
    let now = Instant::now();
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings { pad: true, ..Settings::DEFAULT }, now);
    for (len, body) in [(0, 0), (1, 128), (100, 128), (300, 512), (1000, 1024), (2000, 2048), (3000, 4096)] {
        let sent = vec![0x5a; len];
        let sealed = a.seal(now, DATA, 0, 0, &sent).unwrap();
        assert_eq!(sealed.len(), HEADER + body + crate::TAG, "a payload of {len}");
        let header = Header::parse(&sealed).unwrap();
        assert_eq!((header.length as usize, usize::from(header.pad)), (body, body - len));
        assert_eq!(payload(b.open(now, &sealed)), sent);
    }
    let (mut a, _) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    assert_eq!(a.seal(now, DATA, 0, 0, &[1; 100]).unwrap().len(), HEADER + 100 + crate::TAG, "padding off");
}

#[test]
fn the_type_and_the_flag_of_the_layer_and_a_large_payload_are_refused() {
    let now = Instant::now();
    let (mut a, _) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    assert!(matches!(a.seal(now, RESUME, 0, 0, b""), Err(Fault::Malformed(_))));
    assert!(matches!(a.seal(now, DATA, PHASE, 0, b""), Err(Fault::Malformed(_))));
    assert!(matches!(a.seal(now, DATA, 0, 0, &vec![0; MAX_LENGTH + 1]), Err(Fault::Malformed(_))));
    assert_eq!(a.counters().0, 0, "nothing was sealed");
}

#[test]
fn a_resume_opens_once_and_the_records_after_it_take_the_fresh_prefix() {
    let now = Instant::now();
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let mut entropy = Counting(0);
    for i in 0..3u8 {
        let r = a.seal(now, DATA, 0, 0, &[i]).unwrap();
        if i < 2 {
            payload(b.open(now, &r));
        }
    }
    // The carrier is lost with the third record. B waits for counter 2.
    let resume = b.resume(&mut entropy).unwrap();
    assert_eq!(a.open(now, &resume).unwrap(), Opened::Resume { next: 2 });
    assert!(matches!(a.open(now, &resume), Err(Fault::Resume(_))), "a RESUME works once");
    let after = b.seal(now, DATA, 0, 0, b"after").unwrap();
    assert_eq!(payload(a.open(now, &after)), b"after", "B's fresh prefix");
    // A's RESUME is lost with its carrier; the next one opens, and the
    // records after it take its prefix.
    let lost = a.resume(&mut entropy).unwrap();
    let resume = a.resume(&mut entropy).unwrap();
    assert_eq!(b.open(now, &resume).unwrap(), Opened::Resume { next: 1 }, "A opened B's record 0");
    let again = a.seal(now, DATA, 0, 0, &[2]).unwrap();
    assert_eq!(payload(b.open(now, &again)), [2]);
    assert!(matches!(b.open(now, &lost), Err(Fault::Resume(_))), "an older RESUME");
}

#[test]
fn three_failed_resumes_or_a_late_one_end_the_layer() {
    let now = Instant::now();
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let mut entropy = Counting(0);
    let resume = b.resume(&mut entropy).unwrap();
    a.open(now, &resume).unwrap();
    assert!(matches!(a.open(now, &resume), Err(Fault::Resume(_))));
    let mut changed = b.resume(&mut entropy).unwrap();
    changed[HEADER] ^= 1;
    assert!(matches!(a.open(now, &changed), Err(Fault::Resume(_))));
    assert_eq!(a.open(now, &resume).unwrap_err(), Fault::ResumeExpired, "the third failure");
    let r = b.seal(now, DATA, 0, 0, b"x").unwrap();
    assert_eq!(a.open(now, &r).unwrap_err(), Fault::ResumeExpired, "the layer is ended");
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, Settings::DEFAULT, now);
    let resume = b.resume(&mut entropy).unwrap();
    let late = now + Settings::DEFAULT.resume_window + Duration::from_secs(1);
    assert_eq!(a.open(late, &resume).unwrap_err(), Fault::ResumeExpired);
}

#[test]
fn a_resume_takes_the_receiver_to_the_sender_s_epoch() {
    let now = Instant::now();
    let tiny = Settings { rekey_records: 2, ..Settings::DEFAULT };
    let (mut a, mut b) = ends(Suite::ChaCha20Poly1305, tiny, now);
    // Two epochs of A go with the lost carrier: B is two keys behind, with
    // the same phase.
    for _ in 0..5 {
        a.seal(now, DATA, 0, 0, b"lost").unwrap();
    }
    assert_eq!(a.epochs().0, 2);
    let resume = a.resume(&mut Counting(9)).unwrap();
    assert_eq!(b.open(now, &resume).unwrap(), Opened::Resume { next: 0 });
    let r = a.seal(now, DATA, 0, 0, b"found").unwrap();
    assert_eq!(payload(b.open(now, &r)), b"found");
    assert_eq!(b.epochs().1, 2);
}
