use std::time::UNIX_EPOCH;

use super::*;
use crate::crypto::tai64n;
use crate::{COOKIE_REPLY_LEN, Cookie, Keys, MSG_COOKIE, RESPONSE_LEN, Received, Stamps, Tunnel, initiate_with};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn rand32() -> [u8; 32] {
    random::<32>(&mut OsEntropy).unwrap()
}

/// A responder for `n` clients, and the keys of each client: each odd
/// client has a preshared key.
fn setup(n: usize) -> (Responder, Vec<Keys>) {
    let private = rand32();
    let public = x25519_base(&private);
    let clients: Vec<Keys> = (0..n)
        .map(|i| Keys {
            private: rand32(),
            peer_public: public,
            reserved: [0; 3],
            preshared: (i % 2 == 1).then(rand32),
        })
        .collect();
    let peers = clients.iter().map(|k| Peer { public: k.public(), preshared: k.preshared }).collect();
    (Responder::new(private, peers), clients)
}

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([192, 0, 2, 1], port))
}

/// The TAI64N label of `n` seconds after 1970.
fn stamp(n: u64) -> [u8; 12] {
    tai64n(UNIX_EPOCH + secs(n))
}

/// A handshake of `keys` with the timestamp `ts` at `now`: the peer that
/// the responder found, and the session of the initiator.
fn handshake(r: &mut Responder, keys: &Keys, ts: [u8; 12], now: Instant) -> (usize, Session) {
    let init = initiate_with(keys, rand32(), u32::from_le_bytes(rand32()[..4].try_into().unwrap()), ts);
    match r.receive(&init.message, addr(1000), false, now) {
        Incoming::Response { peer, message } => {
            assert_eq!(message.len(), RESPONSE_LEN);
            (peer, init.finish(keys, &message, now).unwrap())
        }
        other => panic!("{other:?}"),
    }
}

fn packet(tag: u8) -> Vec<u8> {
    let mut p = vec![0x45, 0, 0, 20];
    p.resize(20, tag);
    p
}

/// Three initiators of the crate, one with a preshared key, against one
/// responder at once: each session is found by its receiver index, sends
/// only after the first message of its peer, and does not open the
/// messages of another peer. A rekey of the initiator's `Tunnel` makes a
/// new session; the old one carries until the peer sends in the new one.
#[test]
fn several_peers_at_once_with_the_initiator_of_the_crate() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(3);
    let mut sessions: Vec<Session> = Vec::new();
    for (i, keys) in clients.iter().enumerate() {
        let (peer, session) = handshake(&mut r, keys, stamp(1_800_000_000), t0);
        assert_eq!(peer, i);
        sessions.push(session);
    }
    assert!(matches!(r.send(0, &[packet(1)], t0), Err(Error::NoSession)), "no confirmation yet");
    for i in [2, 0, 1] {
        let m = sessions[i].seal(&[0; 3], &packet(i as u8));
        assert!(
            matches!(r.receive(&m, addr(1000), false, t0), Incoming::Packet { peer, packet: p } if peer == i && p == packet(i as u8))
        );
        assert_eq!(r.session(i).unwrap().remote, sessions[i].local);
    }
    for (i, session) in sessions.iter_mut().enumerate() {
        let out = r.send(i, &[packet(10 + i as u8)], t0).unwrap();
        assert_eq!(session.open(&out[0]).unwrap(), packet(10 + i as u8));
    }
    let mut stray = sessions[0].seal(&[0; 3], &packet(7));
    stray[4..8].copy_from_slice(&sessions[1].remote.to_le_bytes());
    assert!(matches!(r.receive(&stray, addr(1000), false, t0), Incoming::Dropped(Error::NotAuthentic)));

    let wall = UNIX_EPOCH + secs(1_800_000_001);
    let mut tunnel = Tunnel::new(clients[0].clone(), sessions.remove(0), Stamps::new(wall, t0), t0);
    let init = tunnel.rekey(t0 + secs(121)).unwrap().expect("a rekey");
    let Incoming::Response { peer: 0, message } = r.receive(&init, addr(1000), false, t0 + secs(121)) else {
        panic!("a response to the rekey");
    };
    assert!(matches!(tunnel.receive(&message, t0 + secs(121)), Received::Rekeyed));
    let old = r.send(0, &[packet(20)], t0 + secs(122)).unwrap();
    assert!(matches!(tunnel.receive(&old[0], t0 + secs(122)), Received::Packet(p) if p == packet(20)));
    let new = tunnel.send(&[packet(21)], t0 + secs(123)).unwrap();
    assert!(matches!(r.receive(&new.messages[0], addr(1000), false, t0 + secs(123)), Incoming::Packet { peer: 0, .. }));
    let back = r.send(0, &[packet(22)], t0 + secs(123)).unwrap();
    assert_eq!(Session::receiver_of(&back[0]), Some(tunnel.current().local), "the new session");
    assert!(matches!(tunnel.receive(&back[0], t0 + secs(123)), Received::Packet(p) if p == packet(22)));
}

/// An initiation whose timestamp is not after the last one taken from its
/// peer gets no answer: the same bytes again, an older or an equal stamp.
/// A newer one is taken, but not within `INITIATION_GAP` of the last.
#[test]
fn a_replayed_initiation_is_refused() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(1);
    let keys = &clients[0];
    let first = initiate_with(keys, rand32(), 1, stamp(100));
    assert!(matches!(r.receive(&first.message, addr(1), false, t0), Incoming::Response { .. }));
    let again = r.receive(&first.message, addr(1), false, t0 + secs(1));
    assert!(matches!(again, Incoming::Dropped(Error::OldTimestamp)), "{again:?}");
    for ts in [stamp(99), stamp(100)] {
        let init = initiate_with(keys, rand32(), 2, ts);
        assert!(matches!(
            r.receive(&init.message, addr(1), false, t0 + secs(2)),
            Incoming::Dropped(Error::OldTimestamp)
        ));
    }
    let (_, mut session) = handshake(&mut r, keys, stamp(101), t0 + secs(3));
    let soon = initiate_with(keys, rand32(), 3, stamp(102));
    let at = t0 + secs(3) + INITIATION_GAP / 2;
    assert!(matches!(r.receive(&soon.message, addr(1), false, at), Incoming::Dropped(Error::Flood)));
    // A refused initiation leaves the session as it was.
    let m = session.seal(&[0; 3], &packet(1));
    assert!(matches!(r.receive(&m, addr(1), false, at), Incoming::Packet { peer: 0, .. }));
    let at = t0 + secs(3) + INITIATION_GAP;
    assert!(matches!(r.receive(&soon.message, addr(1), false, at), Incoming::Response { .. }));
}

/// A key that is not a peer gets no answer, as a bad mac1, a short
/// initiation, a response, an empty datagram and a transport message to no
/// session; the peers still take their handshakes. `Debug` shows no key.
#[test]
fn an_unknown_key_gets_nothing() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(2);
    let stranger = Keys { private: rand32(), ..clients[0].clone() };
    let init = initiate_with(&stranger, rand32(), 9, stamp(100));
    let got = r.receive(&init.message, addr(1), false, t0);
    assert!(matches!(got, Incoming::Dropped(Error::UnknownPeer)), "{got:?}");
    let mut bad = initiate_with(&clients[0], rand32(), 9, stamp(100)).message.clone();
    bad[120] ^= 1;
    assert!(matches!(r.receive(&bad, addr(1), false, t0), Incoming::Dropped(Error::InitiationMac1)));
    let mut body = initiate_with(&clients[0], rand32(), 9, stamp(100)).message.clone();
    body[50] ^= 1;
    let mac = mac1(&r.public(), &body[..116]);
    body[116..132].copy_from_slice(&mac);
    assert!(matches!(r.receive(&body, addr(1), false, t0), Incoming::Dropped(Error::InitiationNotAuthentic)));
    assert!(matches!(r.receive(&bad[..147], addr(1), false, t0), Incoming::Dropped(Error::NotInitiation)));
    assert!(matches!(r.receive(&[2; 92], addr(1), false, t0), Incoming::Dropped(Error::NotForResponder)));
    assert!(matches!(r.receive(&[], addr(1), false, t0), Incoming::Dropped(Error::NotForResponder)));
    assert!(matches!(r.receive(&[4; 31], addr(1), false, t0), Incoming::Dropped(Error::NotTransport)));
    assert!(matches!(r.receive(&[4; 48], addr(1), false, t0), Incoming::Dropped(Error::OtherSession)));
    for (i, keys) in clients.iter().enumerate() {
        assert_eq!(handshake(&mut r, keys, stamp(100), t0).0, i);
    }
    assert_eq!(format!("{r:?}"), "Responder { peers: 2, .. }");
    assert_eq!(format!("{:?}", Peer { public: [1; 32], preshared: Some([2; 32]) }), "Peer { .. }");
}

/// Under load, an initiation with no mac2 gets a cookie reply, and the
/// retry with the mac2 of its cookie gets the response. The cookie is of
/// one address, and of the secret of 120 s. With no load, mac2 is not read.
#[test]
fn a_cookie_under_load_and_the_retry_with_mac2() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(1);
    let keys = &clients[0];
    let first = initiate_with(keys, rand32(), 1, stamp(100));
    let Incoming::CookieReply(reply) = r.receive(&first.message, addr(1), true, t0) else { panic!("a cookie reply") };
    assert_eq!((reply.len(), reply[0]), (COOKIE_REPLY_LEN, MSG_COOKIE));
    let cookie = Cookie::open(keys, &first, &reply, t0).unwrap();
    let mut elsewhere = initiate_with(keys, rand32(), 2, stamp(101));
    elsewhere.add_mac2(&cookie, t0);
    let got = r.receive(&elsewhere.message, addr(2), true, t0);
    assert!(matches!(got, Incoming::CookieReply(_)), "another address: {got:?}");
    let mut retry = initiate_with(keys, rand32(), 3, stamp(102));
    retry.add_mac2(&cookie, t0);
    let Incoming::Response { peer: 0, message } = r.receive(&retry.message, addr(1), true, t0 + secs(1)) else {
        panic!("the response to the retry");
    };
    assert!(retry.finish(keys, &message, t0 + secs(1)).is_ok());
    let mut late = initiate_with(keys, rand32(), 4, stamp(103));
    late.add_mac2(&cookie, t0 + secs(2));
    let got = r.receive(&late.message, addr(1), true, t0 + COOKIE_SECRET_AGE);
    assert!(matches!(got, Incoming::CookieReply(_)), "a new secret: {got:?}");
    let plain = initiate_with(keys, rand32(), 5, stamp(104));
    assert!(matches!(r.receive(&plain.message, addr(1), false, t0 + secs(130)), Incoming::Response { .. }));
}

/// A session carries until REJECT_AFTER_TIME (180 s) after its handshake,
/// in both ways, and not at that time; `tick` then removes it. A session
/// that waits for its confirmation ends too. The clock is the `now` of the
/// test.
#[test]
fn a_session_expires_after_180_s() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(1);
    let (_, mut session) = handshake(&mut r, &clients[0], stamp(100), t0);
    let m = session.seal(&[0; 3], &packet(1));
    assert!(matches!(r.receive(&m, addr(1), false, t0 + secs(1)), Incoming::Packet { .. }));
    let at = t0 + REJECT_AFTER - secs(1);
    assert_eq!(r.send(0, &[packet(2)], at).unwrap().len(), 1);
    let m = session.seal(&[0; 3], &packet(3));
    assert!(matches!(r.receive(&m, addr(1), false, at), Incoming::Packet { .. }));
    let at = t0 + REJECT_AFTER;
    assert!(matches!(r.send(0, &[packet(4)], at), Err(Error::Expired)));
    let m = session.seal(&[0; 3], &packet(5));
    assert!(matches!(r.receive(&m, addr(1), false, at), Incoming::Dropped(Error::Expired)));
    assert!(r.tick(at).is_empty());
    assert!(matches!(r.receive(&m, addr(1), false, at), Incoming::Dropped(Error::OtherSession)));
    assert!(matches!(r.send(0, &[packet(6)], at), Err(Error::NoSession)));

    let (_, mut waiting) = handshake(&mut r, &clients[0], stamp(200), at);
    let m = waiting.seal(&[0; 3], &packet(7));
    let got = r.receive(&m, addr(1), false, at + REJECT_AFTER);
    assert!(matches!(got, Incoming::Dropped(Error::Expired)), "{got:?}");
}

/// A packet of the peer and nothing out for KEEPALIVE_TIMEOUT (10 s): one
/// keepalive. A send, or a keepalive of the peer, makes none. The responder
/// never sends an initiation, also past REKEY_AFTER_TIME and at the end of
/// its session.
#[test]
fn a_keepalive_when_the_responder_sent_nothing() {
    let t0 = Instant::now();
    let (mut r, clients) = setup(1);
    let (_, mut session) = handshake(&mut r, &clients[0], stamp(100), t0);
    let m = session.seal(&[0; 3], &packet(1));
    assert!(matches!(r.receive(&m, addr(1), false, t0 + secs(1)), Incoming::Packet { .. }));
    assert!(r.tick(t0 + secs(10)).is_empty());
    let out = r.tick(t0 + secs(11));
    assert_eq!(out.len(), 1);
    assert_eq!((out[0].0, out[0].1.len()), (0, 32), "a keepalive is a header and a tag");
    assert_eq!(session.open(&out[0].1).unwrap(), Vec::<u8>::new());
    assert!(r.tick(t0 + secs(12)).is_empty(), "one keepalive");

    let m = session.seal(&[0; 3], &packet(2));
    assert!(matches!(r.receive(&m, addr(1), false, t0 + secs(20)), Incoming::Packet { .. }));
    r.send(0, &[packet(3)], t0 + secs(25)).unwrap();
    assert!(r.tick(t0 + secs(30)).is_empty(), "a send went out");
    let keepalive = session.seal(&[0; 3], &[]);
    assert!(matches!(r.receive(&keepalive, addr(1), false, t0 + secs(40)), Incoming::Keepalive { peer: 0 }));
    assert!(r.tick(t0 + secs(60)).is_empty(), "a keepalive of the peer asks for none");

    let m = session.seal(&[0; 3], &packet(4));
    assert!(matches!(r.receive(&m, addr(1), false, t0 + secs(175)), Incoming::Packet { .. }));
    for s in 100..400 {
        for (_, message) in r.tick(t0 + secs(s)) {
            assert_eq!(message[0], MSG_DATA, "no initiation at {s} s");
        }
    }
    assert!(r.session(0).is_none(), "no session past 180 s");
}
