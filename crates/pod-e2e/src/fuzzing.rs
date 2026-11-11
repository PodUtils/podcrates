//! The readers of the bytes that a peer or a relay sends, public for the
//! fuzz targets of `fuzz/` (feature `fuzzing`). No consumer turns it on.

use crate::net::Held;
use crate::{Ask, Error, Refusal};

/// Each whole frame at the start of `bytes`, as a carry takes them from its
/// buffer, and the bytes that no whole frame took; `Err` at an empty frame.
pub fn take_frames(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>, Option<Error>) {
    let mut buf = bytes.to_vec();
    let mut frames = Vec::new();
    loop {
        match crate::channel::take_frame(&mut buf) {
            Ok(Some(frame)) => frames.push(frame),
            Ok(None) => return (frames, buf, None),
            Err(e) => return (frames, buf, Some(e)),
        }
    }
}

/// What a verdict's body says: let in, or why not; or no verdict.
pub type Answer = Result<Result<(), Refusal>, Error>;

/// The answer of a verdict's body, and the seed that it carries.
pub fn verdict(body: &[u8]) -> (Answer, Option<[u8; 32]>) {
    (crate::frame::answer(body), crate::frame::seed_of(body))
}

/// The body of a verdict, with `seed` after the code of one that lets in.
pub fn verdict_body(answer: &Result<(), Refusal>, seed: Option<&[u8; 32]>) -> Vec<u8> {
    crate::frame::verdict_with(answer, seed)
}

/// The net's part of a proof: the route's bytes, unchecked, what the peer
/// holds, and whether its document follows.
pub fn net_part(rest: &[u8]) -> Option<(Option<Vec<u8>>, Option<Held>, bool)> {
    crate::net::decode(rest).map(|(route, held, follows)| (route.map(<[u8]>::to_vec), held, follows))
}

/// The bytes of [`net_part`] again, for a route of UTF-8.
pub fn net_bytes(route: Option<&str>, held: Option<Held>, follows: bool) -> Vec<u8> {
    crate::net::encode(route, held, follows)
}

/// A proof of a key, read as a node or an operator reads the peer's, with
/// `remote_static` as the Noise static key that the handshake proved;
/// `net_ok` when this end offered the net's form.
pub fn proof(payload: &[u8], remote_static: Option<&[u8]>, net_ok: bool) -> Result<Proof, Error> {
    crate::handshake::proof_of(payload, remote_static, net_ok).map(|read| Proof {
        key: read.key,
        ask: read.ask,
        net: read.net,
        grant: read.grant,
    })
}

/// A proof, read.
#[derive(Debug)]
pub struct Proof {
    pub key: pod_identity::PublicKey,
    pub ask: Ask,
    pub net: Option<(Option<Held>, bool)>,
    pub grant: Option<Vec<u8>>,
}
