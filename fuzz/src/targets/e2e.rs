//! The fuzz targets of the end-to-end channel (`pod-e2e`, feature
//! `fuzzing`; new in `pod-e2e`, T-021). The bytes come from the relay (a
//! frame) or from a peer that holds any key (a verdict, a proof, after
//! Noise opened them), or from the binary before this one (a handover).

use pod_e2e::fuzzing::{Answer, net_bytes, net_part, proof, take_frames, verdict, verdict_body};
use pod_e2e::{Ask, MAX_GRANT, Refusal};

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
