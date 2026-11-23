//! The channel after its handshake. Each message is encrypted with the next
//! nonce of its direction, counted by each end from 0: a message that is
//! changed, dropped, repeated or out of order fails its check, and ends the
//! session.
//!
//! Each direction ends with `END`. The stream under the channel is shut only
//! when both directions have ended: the resumable layer ends a session when
//! one side's bytes end, so a shut at the first `END` would end the session
//! before the other direction's last bytes. A session over the channel thus
//! has the half-close of TCP, and a stream that ends with no `END` is a cut.
//!
//! Between two ends that said they read the net's document, a message `NET`
//! carries one; it goes to the binary's [`Docs`], never to the session's
//! bytes. A verdict that lets in can carry the seed of the session's tokens
//! of one leg, which the operator's end keeps.
//!
//! A node's carry stops between two messages when a handover asks
//! ([`carry`]), and the next binary goes on from the same nonces, with the
//! keys that the handshake split.
//!
//! The carry says each write that takes no byte for 20 s, to the
//! application or to the layer, once for each wait ([`stall`]).
//!
//! In place of the carry, the two ends can agree on a suite of records
//! ([`Channel::agree`]: one message `SUITES` each way), take the records
//! from the exporter of the handshake ([`Channel::exporter`],
//! [`crate::record`]), and take the stream back ([`Channel::into_transport`]).

mod carry;
mod stall;

use std::sync::Arc;

use pod_core::Events;
use sha2::{Digest, Sha256};
use snow::StatelessTransportState;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

#[cfg(feature = "fuzzing")]
pub(crate) use carry::take_frame;
pub use carry::{Carried, Held, Start};

use crate::exporter::Exporter;
use crate::frame::{self, END, NET, SUITES, VERDICT};
use crate::net::Docs;
use crate::record::Suite;
use crate::{Error, HANDSHAKE_LIMIT, MAX_DATA, MAX_MESSAGE, Refusal, TAG};
use pod_identity::PublicKey;

/// The pair that a relay advertises to the two ends of a session: the id of
/// the relay and the id of the pair. [`Channel::bind`] mixes it into the
/// exporter, and [`crate::envelope`] puts it in the associated data of the
/// destination envelope, so a relay that advertises two pairs to the two
/// ends of one session cannot splice them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pair {
    /// The id of the relay.
    pub relay: [u8; 16],
    /// The id of the pair at that relay.
    pub pair: [u8; 16],
}

impl Pair {
    pub fn new(relay: [u8; 16], pair: [u8; 16]) -> Pair {
        Pair { relay, pair }
    }

    /// A pair from two names, which are hashed to 16 bytes each: for a
    /// caller that has the relay's text and not its bytes.
    pub fn named(relay: &str, pair: &str) -> Pair {
        let mut relay_id = [0u8; 16];
        relay_id.copy_from_slice(&Sha256::digest(relay.as_bytes())[..16]);
        let mut pair_id = [0u8; 16];
        pair_id.copy_from_slice(&Sha256::digest(pair.as_bytes())[..16]);
        Pair { relay: relay_id, pair: pair_id }
    }

    /// The 32 bytes that the binding mixes and the envelope authenticates.
    pub fn bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[..16].copy_from_slice(&self.relay);
        out[16..].copy_from_slice(&self.pair);
        out
    }
}

/// The channel over `transport`, after its handshake.
pub struct Channel<T> {
    transport: T,
    state: StatelessTransportState,
    /// The nonce of the next message out.
    sent: u64,
    /// The nonce of the next message in.
    received: u64,
    /// Where a document of the net from the peer goes, and the peer's key,
    /// when both ends read them.
    net: Option<(Arc<dyn Docs>, PublicKey)>,
    /// The seed of the session's tokens of one leg that the node's verdict
    /// gave, at the operator's end.
    seed: Option<Zeroizing<[u8; 32]>>,
    /// The keys that the handshake split, the initiator's then the
    /// responder's, at a node's end: what a handover gives the next binary.
    keys: Option<Zeroizing<[u8; 64]>>,
    /// The exporter of the handshake; none on a channel that a handover
    /// gave, which only its carry takes.
    exporter: Option<Exporter>,
    /// The pair that the relay advertises, once [`Channel::bind`] set it.
    pair: Option<Pair>,
    /// Where the carry says each wait that holds ([`stall`]); with none, it
    /// says nothing.
    events: Option<Arc<dyn Events>>,
}

impl<T> Channel<T> {
    /// Bind the session to the pair that the relay advertises: its id and
    /// the id of the pair. The exporter takes the pair into its secret, so
    /// each key that follows (the records, [`crate::envelope`]) differs for
    /// each pair, and the envelope authenticates the pair. Call it at both
    /// ends, after the handshake and before the records are made. A channel
    /// that a handover gave has no exporter, and a channel bound to another
    /// pair is refused.
    pub fn bind(&mut self, pair: Pair) -> Result<(), Error> {
        if self.pair == Some(pair) {
            return Ok(());
        }
        if self.pair.is_some() {
            return Err(Error::Malformed("the channel is bound to another pair".into()));
        }
        let exporter = self
            .exporter
            .as_mut()
            .ok_or_else(|| Error::Malformed("a channel that a handover gave is not bound".into()))?;
        exporter.rebind(&pair.bytes())?;
        self.pair = Some(pair);
        Ok(())
    }

    /// The pair that the relay advertises, once [`Channel::bind`] set it.
    pub fn pair(&self) -> Option<Pair> {
        self.pair
    }

    /// The exporter of the handshake: keys that only the two ends hold.
    pub fn exporter(&self) -> Option<&Exporter> {
        self.exporter.as_ref()
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> Channel<T> {
    pub(crate) fn new(transport: T, state: StatelessTransportState, exporter: Exporter) -> Self {
        let exporter = Some(exporter);
        Channel {
            transport,
            state,
            sent: 0,
            received: 0,
            net: None,
            seed: None,
            keys: None,
            exporter,
            pair: None,
            events: None,
        }
    }

    /// This channel, with the keys of its handshake.
    pub(crate) fn with_keys(mut self, keys: Zeroizing<[u8; 64]>) -> Self {
        self.keys = Some(keys);
        self
    }

    /// A channel that the binary before this one handed over: its transport
    /// from the keys, and its nonces where they were.
    pub(crate) fn restored(
        transport: T,
        state: StatelessTransportState,
        nonces: (u64, u64),
        keys: Zeroizing<[u8; 64]>,
    ) -> Self {
        let (sent, received) = nonces;
        let keys = Some(keys);
        Channel {
            transport,
            state,
            sent,
            received,
            net: None,
            seed: None,
            keys,
            exporter: None,
            pair: None,
            events: None,
        }
    }

    /// This channel, whose carry says each of its waits that holds
    /// [`pod_session::pump::STALL_AFTER`] to `events`, as an event of the kind
    /// [`crate::ends::EVENT_WAIT`].
    pub fn with_events(mut self, events: Arc<dyn Events>) -> Self {
        self.events = Some(events);
        self
    }

    /// Both ends read the net's documents: one from the peer goes to `docs`.
    pub(crate) fn set_net(&mut self, docs: Arc<dyn Docs>, peer: PublicKey) {
        self.net = Some((docs, peer));
    }

    /// Send this end's document of the net, of [`MAX_DATA`] bytes at most.
    pub(crate) async fn send_net(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > MAX_DATA {
            return Err(Error::Malformed(format!("a document of the net of {} bytes", bytes.len())));
        }
        send(&self.state, &mut self.sent, &mut self.transport, NET, bytes).await
    }

    /// The peer's document of the net, which its proof said would follow.
    pub(crate) async fn read_net(&mut self) -> Result<Vec<u8>, Error> {
        let (mut cipher, mut plain) = (Vec::new(), vec![0u8; MAX_MESSAGE]);
        match receive(&self.state, &mut self.received, &mut self.transport, &mut cipher, &mut plain).await? {
            None => Err(Error::Cut),
            Some((NET, n)) => Ok(plain[1..n].to_vec()),
            Some((kind, _)) => Err(Error::Malformed(format!("a message of type {kind} where the net's document was"))),
        }
    }

    pub(crate) async fn send_verdict(&mut self, answer: &Result<(), Refusal>) -> Result<(), Error> {
        self.send_verdict_with(answer, None).await
    }

    /// [`Channel::send_verdict`], with `seed` in a verdict that lets in: the
    /// seed of the session's tokens of one leg.
    pub(crate) async fn send_verdict_with(
        &mut self,
        answer: &Result<(), Refusal>,
        seed: Option<&[u8; 32]>,
    ) -> Result<(), Error> {
        send(&self.state, &mut self.sent, &mut self.transport, VERDICT, &frame::verdict_with(answer, seed)).await
    }

    /// The node's verdict, the first message after the handshake.
    pub(crate) async fn read_verdict(&mut self) -> Result<(), Error> {
        let (mut cipher, mut plain) = (Vec::new(), vec![0u8; MAX_MESSAGE]);
        match receive(&self.state, &mut self.received, &mut self.transport, &mut cipher, &mut plain).await? {
            None => Err(Error::Cut),
            Some((VERDICT, n)) => {
                // The seed of the session's tokens of one leg, where the node
                // gave one.
                self.seed = frame::seed_of(&plain[1..n]).map(Zeroizing::new);
                frame::answer(&plain[1..n])?.map_err(Error::Refused)
            }
            Some((kind, _)) => Err(Error::Malformed(format!("a message of type {kind} before the verdict"))),
        }
    }

    /// The seed of the session's tokens of one leg that the node's verdict
    /// gave, for the legs that resume the session.
    pub fn leg_seed(&self) -> Option<[u8; 32]> {
        self.seed.as_deref().copied()
    }

    /// The handshake hash of Noise, the binding of this channel: not secret.
    pub fn handshake_hash(&self) -> Option<[u8; 32]> {
        self.exporter.as_ref().map(Exporter::handshake_hash)
    }

    /// Agree with the peer on a suite of records, in place of the carry:
    /// this end sends `offer` (in its order of preference), reads the
    /// peer's, and takes the first suite of the initiator's offer that the
    /// responder offers too. Both ends get the same suite; with none in
    /// common, [`Error::NoSuite`]. A document of the net that comes first
    /// goes to the binary's documents.
    pub async fn agree(&mut self, offer: &[Suite]) -> Result<Suite, Error> {
        if offer.is_empty() || offer.len() > Suite::ALL.len() {
            return Err(Error::Malformed(format!("an offer of {} suites", offer.len())));
        }
        let ids: Vec<u8> = offer.iter().map(|s| s.id()).collect();
        send(&self.state, &mut self.sent, &mut self.transport, SUITES, &ids).await?;
        let theirs = tokio::time::timeout(HANDSHAKE_LIMIT, self.read_suites())
            .await
            .map_err(|_| Error::Timeout("the peer's suites of records"))??;
        let initiator = self.exporter.as_ref().is_some_and(Exporter::is_initiator);
        let (first, second) = if initiator { (offer, &theirs[..]) } else { (&theirs[..], offer) };
        crate::record::agree(first, second).ok_or_else(|| {
            let names = |s: &[Suite]| s.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ");
            Error::NoSuite(format!("this end offers {}; the peer {}", names(offer), names(&theirs)))
        })
    }

    /// The peer's offer of suites; an id that this build does not have is
    /// set aside, as a later release can add a suite.
    async fn read_suites(&mut self) -> Result<Vec<Suite>, Error> {
        let (mut cipher, mut plain) = (Vec::new(), vec![0u8; MAX_MESSAGE]);
        loop {
            match receive(&self.state, &mut self.received, &mut self.transport, &mut cipher, &mut plain).await? {
                None => return Err(Error::Cut),
                Some((SUITES, n)) => return Ok(plain[1..n].iter().filter_map(|id| Suite::of_id(*id)).collect()),
                Some((NET, n)) if self.net.is_some() => {
                    if let Some((docs, peer)) = &self.net {
                        let _ = docs.take(peer, &plain[1..n]);
                    }
                }
                Some((kind, _)) => {
                    return Err(Error::Malformed(format!("a message of type {kind} where the peer's suites were")));
                }
            }
        }
    }

    /// The stream under the channel, for the records that follow
    /// [`Channel::agree`]. The keys of the channel go with it.
    pub fn into_transport(self) -> T {
        self.transport
    }

    /// End both directions at once: a refused session.
    pub(crate) async fn close(mut self) {
        if send(&self.state, &mut self.sent, &mut self.transport, END, &[]).await.is_ok() {
            let _ = self.transport.shutdown().await;
        }
    }

    /// Carry `app` over the channel until both directions end: each byte that
    /// `app` gives goes out encrypted, and each message in is checked, then
    /// goes to `app`, whose writing side is shut at the peer's `END`. An
    /// error of either direction ends both.
    pub async fn carry<A: AsyncRead + AsyncWrite + Unpin>(self, app: A) -> Result<(), Error> {
        // With no hold, the carry ends only when both directions did.
        self.carry_held(app, None, Start::default()).await.map(|_| ())
    }
}

/// Encrypt one message of `kind` with the next nonce out, and write its frame.
async fn send<W: AsyncWrite + Unpin>(
    state: &StatelessTransportState,
    nonce: &mut u64,
    w: &mut W,
    kind: u8,
    body: &[u8],
) -> Result<(), Error> {
    let bytes = sealed(state, nonce, kind, body)?;
    w.write_all(&bytes).await.map_err(Error::Io)?;
    w.flush().await.map_err(Error::Io)
}

/// One message of `kind`, encrypted with the next nonce out: the bytes of
/// its frame.
fn sealed(state: &StatelessTransportState, nonce: &mut u64, kind: u8, body: &[u8]) -> Result<Vec<u8>, Error> {
    let mut plain = Vec::with_capacity(1 + body.len());
    plain.push(kind);
    plain.extend_from_slice(body);
    let mut cipher = vec![0u8; plain.len() + TAG];
    let n = state.write_message(*nonce, &plain, &mut cipher).map_err(|e| Error::Malformed(e.to_string()))?;
    *nonce += 1;
    frame::framed(&cipher[..n])
}

/// Read one frame, and check and decrypt it with the next nonce in: its type
/// and its length in `plain`, type byte included; `None` when the stream
/// ended where a frame would start.
async fn receive<R: AsyncRead + Unpin>(
    state: &StatelessTransportState,
    nonce: &mut u64,
    r: &mut R,
    cipher: &mut Vec<u8>,
    plain: &mut [u8],
) -> Result<Option<(u8, usize)>, Error> {
    if !frame::read_frame(r, cipher).await? {
        return Ok(None);
    }
    if cipher.len() <= TAG {
        return Err(Error::Malformed("a message no longer than its tag".into()));
    }
    // A failed check is all that the channel knows: the relay can have
    // changed, dropped, repeated or moved the message. No part of it is used.
    let n = state.read_message(*nonce, cipher, plain).map_err(|_| Error::Tampered)?;
    *nonce += 1;
    Ok(Some((plain[0], n)))
}
