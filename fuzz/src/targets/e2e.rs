//! The fuzz targets of the end-to-end channel (`pod-e2e`, feature
//! `fuzzing`; new in `pod-e2e`, T-021). The bytes come from the relay (a
//! frame) or from a peer that holds any key (a verdict, a proof, after
//! Noise opened them, a record), or from the binary before this one (a
//! handover).

use std::time::{Duration, Instant};

use pod_e2e::fuzzing::{Answer, net_bytes, net_part, proof, take_frames, verdict, verdict_body};
use pod_e2e::record::{Fault, HEADER, Header, Opened, PHASE, RESUME, Settings, Suite, record_len};
use pod_e2e::{Ask, MAX_GRANT, Refusal, TAG};

use crate::is_one_line;

/// The most bytes of a refusal's reason that a verdict carries.
const MAX_REASON: usize = 1024;

/// The frames of a channel's stream (`pod_e2e::fuzzing::take_frames`), a
/// verdict's body (`verdict`) and the net's part of a proof (`net_part`),
/// each on the same bytes. Each frame has its length and no more, and the
/// frames and the rest are the input; an empty frame is refused. A reason
/// of a refusal is safe on one line, and a seed comes only with a verdict
/// that lets in; a verdict writes back to the same answer. A net's part
/// with a route of UTF-8 writes back to its bytes.
pub fn e2e_frame(data: &[u8]) {
    let (frames, rest, error) = take_frames(data);
    let used: usize = frames.iter().map(|f| 2 + f.len()).sum();
    assert!(frames.iter().all(|f| !f.is_empty() && f.len() <= usize::from(u16::MAX)));
    assert_eq!(used + rest.len(), data.len());
    assert_eq!(&data[used..], &rest[..]);
    if error.is_some() {
        assert_eq!(rest.get(..2), Some(&[0u8, 0][..]), "only an empty frame is refused");
    }

    let (answer, seed) = verdict(data);
    if seed.is_some() {
        assert!(matches!(answer, Ok(Ok(()))), "a seed in a verdict that does not let in");
    }
    if let Ok(said) = &answer {
        if let Err(Refusal::NoTarget(why) | Refusal::Other(why)) = said {
            assert!(is_one_line(why), "{why:?}");
        }
        let short = match said {
            Err(Refusal::NoTarget(why) | Refusal::Other(why)) => why.len() <= MAX_REASON,
            _ => true,
        };
        if short {
            let (again, again_seed): (Answer, _) = verdict(&verdict_body(said, seed.as_ref()));
            assert_eq!(again.ok().as_ref(), Some(said));
            assert_eq!(again_seed, seed);
        }
    }

    if let Some((route, held, follows)) = net_part(data) {
        let route = route.map(String::from_utf8);
        if let Some(Ok(route)) = &route {
            assert!(!route.is_empty(), "a route of no byte reads as none");
        }
        match route {
            None => assert_eq!(net_bytes(None, held, follows), data),
            Some(Ok(route)) => assert_eq!(net_bytes(Some(&route), held, follows), data),
            Some(Err(_)) => {}
        }
    }
}

/// A proof of a key (`pod_e2e::fuzzing::proof`), as an end reads the
/// peer's after Noise opened it. The first byte of the input says whether
/// this end reads the net's form (bit 0) and whether the handshake gave no
/// Noise static key (bit 1); the static key is that of the identity of the
/// seed `[1; 32]`. A proof that reads is of that identity's key alone, as no
/// other key signs its Noise key; its word reads back to its ask; its grant
/// has 1 to `MAX_GRANT` bytes; the net's form comes only where this end
/// reads it.
pub fn e2e_proof(data: &[u8]) {
    let Some((&flags, payload)) = data.split_first() else { return };
    let identity = pod_identity::Identity::from_seed(&[1; 32]);
    let net_ok = flags & 1 == 1;
    let remote = (flags & 2 == 0).then_some(&identity.dh_public()[..]);
    let Ok(read) = proof(payload, remote, net_ok) else { return };
    assert!(remote.is_some(), "a proof with no Noise static key");
    assert_eq!(read.key, identity.public());
    assert_eq!(Ask::read(read.ask.word().as_deref()).as_ref(), Some(&read.ask));
    if let Some(grant) = &read.grant {
        assert!((1..=MAX_GRANT).contains(&grant.len()), "a grant of {} bytes", grant.len());
    }
    assert!(net_ok || read.net.is_none(), "the net's form where this end does not read it");
}

/// The state of a node's channel that the binary before this one handed
/// over (`pod_e2e::hold::ChannelState::decode`): a state that reads encodes
/// to bytes that read to the same state.
pub fn e2e_handover(data: &[u8]) {
    use pod_e2e::hold::ChannelState;
    if let Ok(state) = ChannelState::decode(data) {
        let bytes = state.encode();
        let again = ChannelState::decode(&bytes).expect("an encoded state reads");
        assert!(again == state, "{state:?}");
        assert_eq!(*again.encode(), *bytes);
    }
}

/// The time between two records of [`e2e_record`], which bits 1 and 2 of
/// its first byte pick: the last is past the window of a RESUME.
const STEPS: [u64; 4] = [0, 1, 300, 660];

/// The records that a peer sends (`pod_e2e::record::{record_len, Records::open}`),
/// as a carrier gives them, at one end of an exporter of fixed keys
/// (`pod_e2e::fuzzing::records`). The first byte picks the suite (bit 0),
/// the time between two records (bits 1 and 2, [`STEPS`]), one failed
/// RESUME as the limit (bit 3) and the end that opens (bit 4: the
/// initiator's); the rest is the stream of records, each as long as
/// `record_len` says. A record that opens has the fields of its header and
/// a payload of its `length - pad` bytes, and moves the next counter to
/// after its own and the key at most one epoch on. A record that fails
/// leaves the counters and the epochs as they were, and counts a repeat
/// only as a repeat. After the end of the layer, each record fails so.
pub fn e2e_record(data: &[u8]) {
    let Some((&flags, mut rest)) = data.split_first() else { return };
    let suite = if flags & 1 == 0 { Suite::ChaCha20Poly1305 } else { Suite::Aes256Gcm };
    let step = Duration::from_secs(STEPS[usize::from((flags >> 1) & 3)]);
    let settings = Settings { resume_failures: if flags & 8 == 0 { 3 } else { 1 }, ..Settings::DEFAULT };
    let mut now = Instant::now();
    let Ok(mut end) = pod_e2e::fuzzing::records(flags & 16 != 0, suite, settings, now) else { return };
    let mut ended = false;
    while !rest.is_empty() {
        let len = match record_len(rest) {
            Ok(len) => len,
            Err(_) => {
                assert!(Header::parse(rest).is_err(), "record_len refused a header that reads");
                rest.len()
            }
        };
        let (record, after) = rest.split_at(len.min(rest.len()));
        rest = after;
        now += step;
        let (counters, epochs, replayed) = (end.counters(), end.epochs(), end.replayed());
        let opened = end.open(now, record);
        assert!(!ended || opened == Err(Fault::ResumeExpired), "{opened:?} after the end of the layer");
        assert_eq!(end.counters().0, counters.0, "an open moved the counter of the sealing side");
        assert_eq!(end.epochs().0, epochs.0, "an open moved the key of the sealing side");
        match opened {
            Ok(Opened::Record(r)) => {
                let header = Header::parse(record).expect("a record that opens has a header");
                assert_eq!(record.len(), HEADER + header.length as usize + TAG);
                let unpadded = header.length as usize - usize::from(header.pad);
                assert_eq!(r.payload.len(), unpadded, "the payload is the length of a record less its padding");
                let fields = (r.kind, r.flags, r.stream, r.counter);
                assert_eq!(fields, (header.kind, header.flags & !PHASE, header.stream, header.counter));
                assert_ne!(r.kind, RESUME, "a RESUME opened as a record of the caller");
                assert_eq!(end.counters().1, counters.1.max(r.counter.saturating_add(1)));
                assert!((epochs.1..=epochs.1 + 1).contains(&end.epochs().1), "{epochs:?} to {:?}", end.epochs());
                assert_eq!(end.replayed(), replayed);
            }
            Ok(Opened::Resume { .. }) => {
                assert_eq!(record.get(1), Some(&RESUME), "a record of the caller opened as a RESUME");
                assert_eq!(end.counters(), counters, "a RESUME moved the window");
                assert!(end.epochs().1 >= epochs.1, "a RESUME took the key back");
                assert_eq!(end.replayed(), replayed);
            }
            Err(fault) => {
                assert_eq!((end.counters(), end.epochs()), (counters, epochs), "{fault}");
                let repeat = u64::from(matches!(fault, Fault::Replayed(_)));
                assert_eq!(end.replayed(), replayed + repeat, "{fault}");
                ended |= fault == Fault::ResumeExpired;
            }
        }
    }
}
