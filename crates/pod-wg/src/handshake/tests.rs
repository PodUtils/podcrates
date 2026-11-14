use std::time::Instant;

use pod_core::entropy::OsEntropy;

use super::*;
use crate::blake2s::hex;
use crate::crypto::{open, seal};
use crate::standin;

fn random32() -> [u8; 32] {
    random::<32>(&mut OsEntropy).unwrap()
}

#[test]
fn a_handshake_and_transport_messages() {
    let peer_private = random32();
    let keys =
        Keys { private: random32(), peer_public: x25519_base(&peer_private), reserved: [1, 2, 3], preshared: None };
    let init = initiate(&keys).unwrap();
    assert_eq!(init.message.len(), INITIATION_LEN);
    assert_eq!(&init.message[..4], &[MSG_INITIATION, 1, 2, 3]);
    // mac1 is the MAC of WireGuard over the bytes with zero reserved
    // bytes (WARP writes them after the MAC).
    let mut plain = init.message[..116].to_vec();
    plain[1..4].fill(0);
    let key = hash(&[LABEL_MAC1, &keys.peer_public]);
    assert_eq!(crate::blake2s::blake2s(16, &key, &[&plain]), init.message[116..132]);
    let (response, peer_send, peer_recv, peer_index) = standin::respond(&peer_private, &keys.public(), &init.message);
    assert_eq!(response.len(), RESPONSE_LEN);
    let mut session = init.finish(&keys, &response, Instant::now()).unwrap();
    assert_eq!(session.remote, peer_index);
    assert_eq!(session.send_key, peer_recv);
    assert_eq!(session.recv_key, peer_send);
    // A packet out: an IPv4 header of 28 bytes, padded to 32.
    let mut packet = vec![0x45, 0, 0, 28];
    packet.resize(28, 9);
    let m = session.seal(&keys.reserved, &packet);
    assert_eq!(&m[..4], &[MSG_DATA, 1, 2, 3]);
    assert_eq!(m.len(), 16 + 32 + 16);
    let plain = open(&peer_recv, 0, &m[16..], &[]).unwrap();
    assert_eq!(&plain[..28], &packet[..]);
    // A packet in, then the same message again (a replay).
    let mut back = vec![MSG_DATA, 0, 0, 0];
    back.extend_from_slice(&session.local.to_le_bytes());
    back.extend_from_slice(&5u64.to_le_bytes());
    back.extend_from_slice(&seal(&peer_send, 5, &plain, &[]));
    assert_eq!(session.open(&back).unwrap(), packet);
    assert!(session.open(&back).is_err());
    // A changed byte does not authenticate.
    let mut bad = back.clone();
    bad[8] = 6;
    bad[20] ^= 1;
    assert!(session.open(&bad).is_err());
    // A wrong response key: refused.
    let other = Keys { private: random32(), ..keys.clone() };
    assert!(init.finish(&other, &response, Instant::now()).is_err());
}

/// A response is taken only with the mac1 of this side's key. The reserved
/// bytes and mac2 are outside each check, as WARP writes the reserved bytes
/// after the MAC, and an initiator has no cookie to check a mac2 with.
#[test]
fn a_response_with_a_wrong_mac1_is_refused() {
    let (keys, peer_private) = standin::keys([4, 5, 6]);
    let init = initiate_with(&keys, [8; 32], 21, [0; 12]);
    let (response, ..) = standin::respond_with(&peer_private, &keys.public(), &init.message, &[9; 32], 33);
    let now = Instant::now();
    for at in [60, 75] {
        let mut bad = response.clone();
        bad[at] ^= 1;
        assert!(matches!(init.finish(&keys, &bad, now), Err(Error::Mac1)), "byte {at} of mac1");
    }
    let mut reserved = response.clone();
    reserved[1..4].copy_from_slice(&[1, 2, 3]);
    reserved[80] ^= 1;
    assert_eq!(init.finish(&keys, &reserved, now).unwrap().remote, 33);
    // The empty payload is checked after mac1: a response whose mac1 is
    // right for changed bytes still fails there.
    let mut payload = response.clone();
    payload[50] ^= 1;
    let mac = mac1(&keys.public(), &payload[..60]);
    payload[60..76].copy_from_slice(&mac);
    assert!(matches!(init.finish(&keys, &payload, now), Err(Error::ResponseNotAuthentic)));
    let mut other = response.clone();
    other[8] ^= 1;
    assert!(matches!(init.finish(&keys, &other, now), Err(Error::OtherHandshake)));
    assert!(matches!(init.finish(&keys, &response[..91], now), Err(Error::NotResponse)));
}

/// The same values give the same initiation; a dry source of random bytes
/// gives an error, not an initiation.
#[test]
fn an_initiation_from_known_values() {
    let (keys, _) = standin::keys([0; 3]);
    let a = initiate_with(&keys, [1; 32], 7, [2; 12]);
    let b = initiate_with(&keys, [1; 32], 7, [2; 12]);
    assert_eq!(a.message, b.message);
    assert_eq!(a.sender(), 7);
    assert_eq!(a.mac1(), mac1(&keys.peer_public, &a.message[..116]));
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), pod_core::entropy::NoRandom> {
            Err(pod_core::entropy::NoRandom)
        }
    }
    assert!(matches!(initiate_from(&keys, &mut Dry, [0; 12]), Err(Error::NoRandom)));
}

/// A preshared key on both sides gives one session. A key on one side
/// only, or two different keys, fail at the empty payload of the response.
/// The initiation does not depend on the key. `Debug` shows no key.
#[test]
fn a_handshake_with_a_preshared_key() {
    let (plain, peer_private) = standin::keys([0; 3]);
    let keys = Keys { preshared: Some([0x5a; 32]), ..plain.clone() };
    let init = initiate_with(&keys, [8; 32], 21, [1; 12]);
    assert_eq!(init.message, initiate_with(&plain, [8; 32], 21, [1; 12]).message);
    let now = Instant::now();
    let respond = |psk: Option<&[u8; 32]>| {
        standin::respond_preshared(&peer_private, &keys.public(), psk, &init.message, &[9; 32], 33)
    };
    let (response, peer_send, peer_recv, _) = respond(Some(&[0x5a; 32]));
    let session = init.finish(&keys, &response, now).unwrap();
    assert_eq!((session.send_key, session.recv_key), (peer_recv, peer_send));
    let (without, ..) = respond(None);
    assert!(matches!(init.finish(&keys, &without, now), Err(Error::ResponseNotAuthentic)), "only the initiator");
    assert!(matches!(init.finish(&plain, &response, now), Err(Error::ResponseNotAuthentic)), "only the responder");
    let (other, ..) = respond(Some(&[0x5b; 32]));
    assert!(matches!(init.finish(&keys, &other, now), Err(Error::ResponseNotAuthentic)), "another key");
    assert_eq!(format!("{keys:?}"), "Keys { .. }");
}

/// A handshake with a preshared key on fixed values, as an independent
/// program computed it: the Noise IKpsk2 steps written out on Python's
/// hashlib, hmac and `cryptography` (no published WireGuard vector has a
/// preshared key and fixed keys). The same values with the zero key give
/// the response of WireGuard with no preshared key.
#[test]
fn the_vector_of_a_handshake_with_a_preshared_key() {
    let range = |from: u8| -> [u8; 32] { std::array::from_fn(|i| from + i as u8) };
    let (si, sr, ei, er, psk) = (range(0x10), range(0x30), range(0x50), range(0x70), range(0x90));
    let keys = Keys { private: si, peer_public: x25519_base(&sr), reserved: [0; 3], preshared: Some(psk) };
    let timestamp: [u8; 12] = hex("400000006a7e6f00075bcd15").try_into().unwrap();
    let init = initiate_with(&keys, ei, 0x0102_0304, timestamp);
    assert_eq!(
        init.message,
        hex(concat!(
            "0100000004030201392d174a38b3b1beafaf1fe824870841c5fa531bc6eafdb6402c124664488c1c0c866baa",
            "c29f4bf4df0c9d4462d272eeb09d6a1212e247851f727b562292c28be0c55562545967524af7c3bc7aafee5c",
            "227b76cf3041661ea21fbdb4b25f9827aed7ba445e923f40a6bb5bcbd50105ae604fce19439344ef3b76d1e2",
            "00000000000000000000000000000000"
        ))
    );
    let (response, ..) = standin::respond_preshared(&sr, &keys.public(), Some(&psk), &init.message, &er, 0x0a0b_0c0d);
    assert_eq!(
        response,
        hex(concat!(
            "020000000d0c0b0a0403020123b7bb8c91ae008711fb12846780bcdf1e065f821bdfec49f57e7c7dcd4c4823",
            "c198f34790fca6612c1880773f01b7fccdbe9358c20ae423049370502d2fe84100000000000000000000000000000000"
        ))
    );
    let session = init.finish(&keys, &response, Instant::now()).unwrap();
    assert_eq!(session.send_key.to_vec(), hex("45eb39bca38dc232fbca95a631ce0dbdfbd9eb529199a7ff546747d42935c4b7"));
    assert_eq!(session.recv_key.to_vec(), hex("36161ffea40e74c7db414206ae6e6a48e6f0bc5a634fd8186b757281ff720883"));
    assert_eq!((session.local, session.remote), (0x0102_0304, 0x0a0b_0c0d));

    let zero = Keys { preshared: None, ..keys.clone() };
    let (response, ..) = standin::respond_with(&sr, &zero.public(), &init.message, &er, 0x0a0b_0c0d);
    assert_eq!(
        response[44..60].to_vec(),
        hex("646f290fed27ca8a881e6bf3fc7d2b27"),
        "the empty payload with the zero key"
    );
    let session = init.finish(&zero, &response, Instant::now()).unwrap();
    assert_eq!(session.send_key.to_vec(), hex("a8442f5501e3705028170664e1b1d8593b02674770024f343375214e95090c76"));
}
