use std::time::{Duration, SystemTime};

use super::*;
use crate::cookie::mac2;
use crate::handshake::{INITIATION_LEN, initiate_with};
use crate::standin;

/// A tunnel after a handshake with the stand-in at `t0`, and the private key
/// of the stand-in.
fn tunnel(t0: Instant) -> (Tunnel, [u8; 32]) {
    let (keys, peer_private) = standin::keys([1, 2, 3]);
    let init = initiate_with(&keys, [5; 32], 1, [0; 12]);
    let (response, ..) = standin::respond(&peer_private, &keys.public(), &init.message);
    let session = init.finish(&keys, &response, t0).unwrap();
    (Tunnel::new(keys, session, Stamps::new(SystemTime::now(), t0), t0), peer_private)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// The keepalive after 25 s with nothing sent, the rekey after 120 s from
/// the given clock, its resend after 5 s with no answer, and the dead path
/// 20 s after its first initiation. The tunnel reads only the clock that
/// the caller gives.
#[test]
fn the_timers_of_a_tunnel() {
    let t0 = Instant::now();
    let (mut t, _) = tunnel(t0);
    assert!(t.tick(t0 + secs(24)).unwrap().is_empty());
    let out = t.tick(t0 + secs(25)).unwrap();
    assert_eq!(out.len(), 1, "a keepalive");
    assert_eq!(out[0].len(), 32, "a keepalive is a header and a tag");
    assert_eq!(out[0][0], MSG_DATA);
    // A send moves the time of the keepalive.
    let sent = t.send(&[[0x45u8; 20]], t0 + secs(40)).unwrap();
    assert!(sent.handshake.is_none());
    assert_eq!(sent.messages.len(), 1);
    assert!(t.tick(t0 + secs(64)).unwrap().is_empty());
    assert!(!t.rekeying());
    // The rekey: due after REKEY_AFTER of the session.
    assert_eq!(t.rekey(t0 + secs(120)).unwrap(), None);
    let first = t.rekey(t0 + secs(121)).unwrap().expect("an initiation");
    assert_eq!(first.len(), INITIATION_LEN);
    assert!(t.rekeying());
    assert_eq!(t.rekey(t0 + secs(126)).unwrap(), None, "the rekey waits REKEY_TIMEOUT");
    let again = t.send(&[[0x45u8; 20]], t0 + secs(127)).unwrap().handshake.expect("a resend");
    assert_ne!(again, first, "a new initiation");
    assert!(again[88..116] != first[88..116], "a new timestamp");
    assert!(t.rekey(t0 + secs(141)).unwrap().is_some(), "the next resend; the first initiation was 20 s ago");
    assert!(matches!(t.tick(t0 + secs(142)), Err(Error::Dead(d)) if d == HANDSHAKE_DEAD));
}

/// The response of a rekey makes a new current session; a transport
/// message to the previous one still opens.
#[test]
fn a_rekey_takes_its_response_and_keeps_the_previous_session() {
    let t0 = Instant::now();
    let (mut t, peer_private) = tunnel(t0);
    let old_local = t.current().local;
    let old_key = t.current().recv_key;
    let init = t.rekey(t0 + secs(121)).unwrap().unwrap();
    let (response, peer_send, _, peer_index) = standin::respond(&peer_private, &t.keys().public(), &init);
    assert!(matches!(t.receive(&response, t0 + secs(122)), Received::Rekeyed));
    assert!(!t.rekeying());
    assert_eq!(t.current().remote, peer_index);
    assert!(matches!(t.receive(&response, t0 + secs(122)), Received::Ignored), "no rekey waits");
    // A packet to the new session, and one to the previous session.
    let mut packet = vec![0x45, 0, 0, 20];
    packet.resize(20, 1);
    let mut new = standin::session(peer_send);
    new.remote = t.current().local;
    let m = new.seal(&[0; 3], &packet);
    assert!(matches!(t.receive(&m, t0 + secs(123)), Received::Packet(p) if p == packet));
    let mut old = standin::session(old_key);
    old.remote = old_local;
    let m = old.seal(&[0; 3], &packet);
    assert!(matches!(t.receive(&m, t0 + secs(123)), Received::Packet(p) if p == packet));
    let keepalive = old.seal(&[0; 3], &[]);
    assert!(matches!(t.receive(&keepalive, t0 + secs(123)), Received::Keepalive));
    assert!(matches!(t.receive(&m, t0 + secs(123)), Received::Dropped(Error::Replayed(0))));
    let mut stray = old.seal(&[0; 3], &packet);
    stray[4..8].copy_from_slice(&999u32.to_le_bytes());
    assert!(matches!(t.receive(&stray, t0 + secs(123)), Received::Ignored));
    assert!(matches!(t.receive(&[], t0 + secs(123)), Received::Ignored));
}

/// A response that fails its check does not move the times of the rekey: a
/// peer that answers with bad responses cannot hold a dead path open.
#[test]
fn a_bad_response_keeps_the_dead_time() {
    let t0 = Instant::now();
    let (mut t, peer_private) = tunnel(t0);
    let init = t.rekey(t0 + secs(121)).unwrap().unwrap();
    let (mut response, ..) = standin::respond(&peer_private, &t.keys().public(), &init);
    response[50] ^= 1;
    assert!(matches!(t.receive(&response, t0 + secs(125)), Received::Dropped(Error::Mac1)));
    assert!(t.rekeying());
    // The resend comes 5 s after the initiation, not after the response.
    assert!(t.rekey(t0 + secs(127)).unwrap().is_some());
    assert!(matches!(t.receive(&response, t0 + secs(140)), Received::Dropped(_)));
    assert!(matches!(t.rekey(t0 + secs(142)), Err(Error::Dead(_))), "20 s after the first initiation");
}

/// A cookie reply to the rekey gives mac2 to its resend, and not before:
/// the resend waits for its time. A reply to no rekey is not taken.
#[test]
fn a_cookie_reply_puts_mac2_in_the_resend() {
    let t0 = Instant::now();
    let (mut t, _) = tunnel(t0);
    let peer_public = t.keys().peer_public;
    let early = standin::cookie_reply(&peer_public, &[0; INITIATION_LEN], &[3; 16], &[4; 24]);
    assert!(matches!(t.receive(&early, t0 + secs(1)), Received::Ignored));
    let init = t.rekey(t0 + secs(121)).unwrap().unwrap();
    assert_eq!(init[132..], [0; 16]);
    let reply = standin::cookie_reply(&peer_public, &init, &[3; 16], &[4; 24]);
    assert!(matches!(t.receive(&reply, t0 + secs(122)), Received::Cookie));
    assert_eq!(t.rekey(t0 + secs(122)).unwrap(), None, "no send at once");
    let resend = t.rekey(t0 + secs(127)).unwrap().unwrap();
    assert_eq!(resend[132..], mac2(&[3; 16], &resend[..132]));
    let mut bad = standin::cookie_reply(&peer_public, &resend, &[3; 16], &[4; 24]);
    bad[40] ^= 1;
    assert!(matches!(t.receive(&bad, t0 + secs(128)), Received::Dropped(Error::CookieNotAuthentic)));
}
