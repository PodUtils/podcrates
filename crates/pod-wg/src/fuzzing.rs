//! The readers of the bytes of a peer on fixed keys, public for the fuzz
//! targets of fuzz/ (feature `fuzzing`): a target gives any bytes, and
//! checks the answer against the one message that each reader takes.

use std::time::{Duration, Instant};

use pod_core::entropy::{Entropy, NoRandom};

use crate::{Cookie, Error, Incoming, Initiation, Keys, Peer, Responder, Session, initiate_with, standin};

/// The keys of the initiator, and the private key of the responder.
pub fn keys() -> (Keys, [u8; 32]) {
    let peer_private = [2; 32];
    let keys = Keys {
        private: [1; 32],
        peer_public: crate::crypto::x25519_base(&peer_private),
        reserved: [7, 8, 9],
        preshared: None,
    };
    (keys, peer_private)
}

/// The initiation that each reader answers.
pub fn initiation() -> Initiation {
    initiate_with(&keys().0, [3; 32], 11, [0; 12])
}

/// The response of the responder to [`initiation`]: the one that
/// [`finish`] takes, but for its reserved bytes and its mac2.
pub fn response() -> Vec<u8> {
    let (keys, peer_private) = keys();
    standin::respond_with(&peer_private, &keys.public(), &initiation().message, &[4; 32], 22).0
}

/// Read `bytes` as the response to [`initiation`].
pub fn finish(bytes: &[u8]) -> Result<Session, Error> {
    initiation().finish(&keys().0, bytes, Instant::now())
}

/// The cookie reply to [`initiation`]: the one that [`cookie`] takes, but
/// for its reserved bytes.
pub fn cookie_reply() -> Vec<u8> {
    standin::cookie_reply(&keys().0.peer_public, &initiation().message, &[5; 16], &[6; 24])
}

/// Read `bytes` as a cookie reply to [`initiation`].
pub fn cookie(bytes: &[u8]) -> Result<Cookie, Error> {
    Cookie::open(&keys().0, &initiation(), bytes, Instant::now())
}

/// The key of the transport session of [`open`], both ways; its index is 1.
pub const TRANSPORT_KEY: [u8; 32] = [8; 32];

/// A transport message to the session of [`open`]: `packet` under
/// `counter`.
pub fn transport(packet: &[u8], counter: u64) -> Vec<u8> {
    let mut s = standin::session(TRANSPORT_KEY);
    (s.remote, s.send_counter) = (1, counter);
    s.seal(&[7, 8, 9], packet)
}

/// Read `bytes` twice as a transport message to a new session: the answer
/// of each read.
pub fn open(bytes: &[u8]) -> (Result<Vec<u8>, Error>, Result<Vec<u8>, Error>) {
    let mut s = standin::session(TRANSPORT_KEY);
    let first = s.open(bytes);
    (first, s.open(bytes))
}

/// Random bytes that are the same in each run: a fuzz target must give the
/// same answer to the same input.
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

/// The responder of the private key of [`keys`], with the initiator of
/// [`keys`] as its one peer.
fn responder() -> Responder {
    let (keys, peer_private) = keys();
    let peer = Peer { public: keys.public(), preshared: None };
    Responder::with_entropy(peer_private, vec![peer], Box::new(Counting(0)))
}

/// Read `bytes` as a datagram to a new [`Responder`]: the answer, the answer
/// to the same bytes one second later, and the answer of a new responder
/// under load. [`initiation`] is the one initiation that it answers.
pub fn respond(bytes: &[u8]) -> (Incoming, Incoming, Incoming) {
    let from = std::net::SocketAddr::from(([192, 0, 2, 1], 51820));
    let now = Instant::now();
    let mut r = responder();
    let first = r.receive(bytes, from, false, now);
    let again = r.receive(bytes, from, false, now + Duration::from_secs(1));
    (first, again, responder().receive(bytes, from, true, now))
}
