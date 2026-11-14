//! The fuzz targets of WireGuard (`pod-wg`, feature `fuzzing`; new in
//! `pod-wg`, T-024 and T-562). The bytes come from the network: a response
//! and a cookie reply to this side's initiation, a transport message to a
//! session, and an initiation to the responder. Each reader runs on fixed
//! keys, so that a target knows the one message that it takes.

use pod_wg::fuzzing::{cookie, cookie_reply, finish, initiation, keys, open, respond, response};
use pod_wg::{Cookie, Error, Incoming};

/// True when `data` is `valid`, but for the bytes from `start` to `end`
/// (not included) of each pair of `free`.
fn same_but(data: &[u8], valid: &[u8], free: &[(usize, usize)]) -> bool {
    data.len() == valid.len()
        && data
            .iter()
            .zip(valid)
            .enumerate()
            .all(|(i, (a, b))| a == b || free.iter().any(|&(s, e)| (s..e).contains(&i)))
}

/// A handshake response (`pod_wg::fuzzing::finish`, `Initiation::finish`):
/// it is taken only when it is the responder's response, but for the
/// reserved bytes (1 to 3: WARP writes them after the MAC) and mac2 (76 to
/// 91: an initiator has no cookie to check it with). Each other change is
/// refused.
pub fn wg_response(data: &[u8]) {
    let taken = finish(data);
    assert_eq!(taken.is_ok(), same_but(data, &response(), &[(1, 4), (76, 92)]), "{taken:?}");
    if let Ok(session) = taken {
        assert_eq!((session.local, session.remote), (11, 22));
    }
}

/// A cookie reply (`pod_wg::fuzzing::cookie`, `Cookie::open`): it opens
/// only when it is the responder's reply to this initiation, but for the
/// reserved bytes.
pub fn wg_cookie(data: &[u8]) {
    let taken = cookie(data);
    assert_eq!(taken.is_ok(), same_but(data, &cookie_reply(), &[(1, 4)]), "{taken:?}");
}

/// A transport message (`pod_wg::fuzzing::open`, `Session::open`), read
/// twice by one session: a message that opens has the type 4, the index of
/// the session, a header and a tag around its packet, and its second read
/// is a replay. A message that does not open does not open the second time.
pub fn wg_transport(data: &[u8]) {
    let (first, second) = open(data);
    match first {
        Ok(packet) => {
            assert!(data.len() >= 32 && data[0] == pod_wg::MSG_DATA);
            assert_eq!(pod_wg::Session::receiver_of(data), Some(1));
            assert!(packet.len() <= data.len() - 32);
            let counter = u64::from_le_bytes(data[8..16].try_into().unwrap());
            assert!(matches!(second, Err(Error::Replayed(c)) if c == counter), "{second:?}");
        }
        Err(_) => assert!(second.is_err()),
    }
}

/// An initiation to the responder (`pod_wg::fuzzing::respond`,
/// `Responder::receive`): it gets a response only when it is the initiation
/// of the peer, but for the reserved bytes (1 to 3) and mac2 (132 to 147:
/// the responder is not under load), and the response finishes the
/// handshake. The same bytes again are a replay. Under load, each
/// initiation whose mac1 holds gets a cookie reply, of any key (the reply
/// comes before the static key is read), and no other bytes get an answer;
/// the reply to the initiation of the peer opens.
pub fn wg_initiation(data: &[u8]) {
    let (first, again, loaded) = respond(data);
    let valid = initiation();
    let taken = same_but(data, &valid.message, &[(1, 4), (132, 148)]);
    let mac1_holds = data.len() == pod_wg::INITIATION_LEN
        && data[0] == pod_wg::MSG_INITIATION
        && !matches!(first, Incoming::Dropped(Error::InitiationMac1));
    let now = std::time::Instant::now();
    match first {
        Incoming::Response { peer, message } => {
            assert!(taken);
            assert_eq!(peer, 0);
            valid.finish(&keys().0, &message, now).expect("the response finishes the handshake");
            assert!(matches!(again, Incoming::Dropped(Error::OldTimestamp)), "{again:?}");
        }
        Incoming::Dropped(_) => {
            assert!(!taken);
            assert!(matches!(again, Incoming::Dropped(_)), "{again:?}");
        }
        other => panic!("{other:?}"),
    }
    match loaded {
        Incoming::CookieReply(reply) => {
            assert!(mac1_holds);
            if taken {
                Cookie::open(&keys().0, &valid, &reply, now).expect("the cookie opens");
            }
        }
        Incoming::Dropped(_) => assert!(!mac1_holds),
        other => panic!("under load: {other:?}"),
    }
}
