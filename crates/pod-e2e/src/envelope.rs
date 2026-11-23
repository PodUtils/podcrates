//! The destination envelope: the host, the port and the service name of a
//! session, encoded, padded to one bucket and sealed under a key of the
//! session, before any byte of the application. A relay carries one
//! fixed-size opaque blob and its identifier; it never sees the host, the
//! port or the service name.
//!
//! The key comes from the exporter of the handshake. [`crate::Channel::bind`]
//! mixes the pair that the relay advertises into that exporter, and the
//! envelope authenticates the pair as associated data. Thus a relay that
//! gives the two ends of one session two pairs, or that moves the envelope
//! of one session into another, gets [`EnvelopeError::NotForThisSession`]
//! and no destination.
//!
//! A [`Prelude`] is the blob as a relay carries it: the bytes and their
//! identifier, and no method that gives a [`Destination`]. Only
//! [`Destination::open`] opens it, and that call needs the [`crate::Channel`],
//! so a relay cannot reach the destination without the key.
//!
//! The envelope always uses ChaCha20-Poly1305: it comes before the agreement
//! on the suite of the records. The body is a single bucket of [`BODY`]
//! bytes, so each envelope has the same [`SIZE`].

use std::fmt;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::Channel;
use crate::TAG;
use crate::exporter::Exporter;
use crate::legs::hex;
use crate::record::Suite;
use crate::record::keys::Cipher;

/// The version of an envelope that this build writes and reads.
pub const VERSION: u8 = 1;
/// The bytes of the identifier of an envelope: what a relay routes by.
pub const ID: usize = 16;
/// The bytes of the nonce, which the seal derives and the envelope carries.
pub const NONCE: usize = 12;
/// The bytes of the padded body: the bucket that holds every destination.
pub const BODY: usize = 256;
/// The bytes of a whole envelope: fixed, so that the relay learns no size.
pub const SIZE: usize = 1 + ID + NONCE + BODY + TAG;
/// The offset of the body, and of the tag, in an envelope.
const AT_BODY: usize = 1 + ID + NONCE;
const AT_TAG: usize = AT_BODY + BODY;

/// The label of the key of an envelope.
const KEY: &str = "pod-e2e destination";
/// The label of the identifier of a destination.
const ID_LABEL: &str = "pod-e2e destination id";
/// The label of the nonce of an envelope.
const NONCE_LABEL: &str = "pod-e2e destination nonce";

/// The destination of a session: where the far end dials once the envelope
/// is open. It is never in the clear on the wire, and no relay holds one.
#[derive(Clone, PartialEq, Eq)]
pub struct Destination {
    host: String,
    port: u16,
    service: String,
}

impl Destination {
    /// Seal the destination of `host`, `port` and `service` under the
    /// session of `channel`, which must be bound to the relay's pair. The
    /// blob is the same size for each destination, and the identifier is
    /// derived from the destination and the session key.
    pub fn seal<T>(channel: &Channel<T>, host: &str, port: u16, service: &str) -> Result<Sealed, EnvelopeError> {
        let pair = channel.pair().ok_or(EnvelopeError::Unbound)?;
        let exporter = channel.exporter().ok_or(EnvelopeError::Unbound)?;
        let plaintext = encode(host, port, service)?;
        let mut key = Zeroizing::new([0u8; 32]);
        exporter.expand(KEY, &pair.bytes(), &mut key[..]).map_err(malformed)?;
        let mut id = [0u8; ID];
        let mut nonce = [0u8; NONCE];
        derived(exporter, ID_LABEL, pair, &plaintext, &mut id)?;
        derived(exporter, NONCE_LABEL, pair, &plaintext, &mut nonce)?;
        let mut body = plaintext;
        let tag = Cipher::new(Suite::ChaCha20Poly1305, &key).seal(&nonce, &aad(&id, pair), &mut body);
        let mut bytes = [0u8; SIZE];
        bytes[0] = VERSION;
        bytes[1..1 + ID].copy_from_slice(&id);
        bytes[1 + ID..AT_BODY].copy_from_slice(&nonce);
        bytes[AT_BODY..AT_TAG].copy_from_slice(&body);
        bytes[AT_TAG..].copy_from_slice(&tag);
        Ok(Sealed { bytes })
    }

    /// Open the envelope `blob` of the session of `channel`. A blob of
    /// another session, of another relay pair, or one that was changed on
    /// its way is [`EnvelopeError::NotForThisSession`].
    pub fn open<T>(channel: &Channel<T>, blob: &[u8]) -> Result<Destination, EnvelopeError> {
        if blob.len() != SIZE {
            return Err(EnvelopeError::Malformed(format!(
                "an envelope of {} bytes, where each envelope is {SIZE}",
                blob.len()
            )));
        }
        if blob[0] != VERSION {
            return Err(EnvelopeError::Malformed(format!(
                "an envelope of version {}, where this end reads {VERSION}",
                blob[0]
            )));
        }
        let pair = channel.pair().ok_or(EnvelopeError::Unbound)?;
        let exporter = channel.exporter().ok_or(EnvelopeError::Unbound)?;
        let mut id = [0u8; ID];
        id.copy_from_slice(&blob[1..1 + ID]);
        let mut nonce = [0u8; NONCE];
        nonce.copy_from_slice(&blob[1 + ID..AT_BODY]);
        let tag: [u8; TAG] = blob[AT_TAG..].try_into().map_err(|_| not_for_this_session())?;
        let mut key = Zeroizing::new([0u8; 32]);
        exporter.expand(KEY, &pair.bytes(), &mut key[..]).map_err(malformed)?;
        let mut body = [0u8; BODY];
        body.copy_from_slice(&blob[AT_BODY..AT_TAG]);
        if !Cipher::new(Suite::ChaCha20Poly1305, &key).open(&nonce, &aad(&id, pair), &mut body, &tag) {
            return Err(not_for_this_session());
        }
        let destination = decode(&body)?;
        let mut want = [0u8; ID];
        derived(exporter, ID_LABEL, pair, &body, &mut want)?;
        if want != id {
            return Err(EnvelopeError::Malformed("an identifier that does not follow the destination".into()));
        }
        Ok(destination)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    /// The destination as `host:port`, which the far end dials.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// A destination that this end does not print, so that a line or a log of
/// the far end does not put it back in the clear.
impl fmt::Debug for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Destination { sealed }")
    }
}

/// A sealed destination: the fixed bytes that go on the wire.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    bytes: [u8; SIZE],
}

impl Sealed {
    /// The identifier of this envelope: what a relay routes by.
    pub fn id(&self) -> [u8; ID] {
        let mut out = [0u8; ID];
        out.copy_from_slice(&self.bytes[1..1 + ID]);
        out
    }

    pub fn bytes(&self) -> &[u8; SIZE] {
        &self.bytes
    }

    /// The same bytes as a prelude, for the relay that carries them.
    pub fn prelude(&self) -> Prelude {
        Prelude { bytes: self.bytes }
    }
}

/// No destination and no key in a print.
impl fmt::Debug for Sealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sealed {{ id: {}, {SIZE} bytes }}", hex(&self.id()))
    }
}

/// The opaque prelude that a relay carries: the fixed bytes and their
/// identifier. It has no method that gives a [`Destination`], and it holds
/// no key.
///
/// ```compile_fail
/// use pod_e2e::envelope::Prelude;
/// fn reach(prelude: Prelude) {
///     let _ = prelude.destination();
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Prelude {
    bytes: [u8; SIZE],
}

impl Prelude {
    /// The prelude of `bytes`, which must be one whole envelope.
    pub fn of(bytes: &[u8]) -> Result<Prelude, EnvelopeError> {
        let bytes: [u8; SIZE] = bytes.try_into().map_err(|_| {
            EnvelopeError::Malformed(format!("a prelude of {} bytes, where one is {SIZE}", bytes.len()))
        })?;
        Ok(Prelude { bytes })
    }

    /// Read one whole prelude from `r`.
    pub async fn take<R: AsyncRead + Unpin>(r: &mut R) -> Result<Prelude, EnvelopeError> {
        let mut bytes = [0u8; SIZE];
        r.read_exact(&mut bytes).await.map_err(EnvelopeError::Io)?;
        Ok(Prelude { bytes })
    }

    /// Write this prelude whole to `w`.
    pub async fn pass<W: AsyncWrite + Unpin>(&self, w: &mut W) -> Result<(), EnvelopeError> {
        w.write_all(&self.bytes).await.map_err(EnvelopeError::Io)?;
        w.flush().await.map_err(EnvelopeError::Io)
    }

    /// The identifier of this prelude: what a relay routes by.
    pub fn id(&self) -> [u8; ID] {
        let mut out = [0u8; ID];
        out.copy_from_slice(&self.bytes[1..1 + ID]);
        out
    }

    pub fn bytes(&self) -> &[u8; SIZE] {
        &self.bytes
    }
}

impl fmt::Debug for Prelude {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Prelude {{ id: {}, {SIZE} bytes }}", hex(&self.id()))
    }
}

/// The relay of an opaque prelude. It reads a whole prelude, routes it by
/// its identifier, and writes it to the far end; it never opens it, and no
/// call of this type takes a key or gives a destination.
#[derive(Debug, Default, Clone, Copy)]
pub struct Forwarder;

impl Forwarder {
    pub fn new() -> Forwarder {
        Forwarder
    }

    /// The identifier that this relay routes by.
    pub fn route(&self, prelude: &Prelude) -> [u8; ID] {
        prelude.id()
    }

    /// Read one prelude from `from`, and write it whole to `to`: the far end
    /// opens it, and the relay holds the opaque bytes only.
    pub async fn forward<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &self,
        from: &mut R,
        to: &mut W,
    ) -> Result<Prelude, EnvelopeError> {
        let prelude = Prelude::take(from).await?;
        prelude.pass(to).await?;
        Ok(prelude)
    }
}

/// Why an envelope did not seal or open.
#[derive(Debug)]
pub enum EnvelopeError {
    /// The blob is not of this session: another key, another relay pair, or
    /// a byte that changed on its way.
    NotForThisSession,
    /// The channel is not bound to the relay's pair, or has no exporter.
    Unbound,
    /// A destination or an envelope that the layer does not have.
    Malformed(String),
    /// A destination above the one bucket.
    TooLong(usize),
    /// The stream of the prelude failed.
    Io(std::io::Error),
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvelopeError::NotForThisSession => f.write_str(
                "the destination envelope is not for this session: another session's key, another advertised pair, \
                 or a byte that changed on its way",
            ),
            EnvelopeError::Unbound => f.write_str("the channel has no relay pair or no exporter to seal with"),
            EnvelopeError::Malformed(why) => write!(f, "a destination envelope that the layer does not have: {why}"),
            EnvelopeError::TooLong(len) => {
                write!(f, "a destination of {len} bytes, above the {BODY} of one envelope")
            }
            EnvelopeError::Io(e) => write!(f, "the stream of the prelude failed: {e}"),
        }
    }
}

impl std::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EnvelopeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for EnvelopeError {
    fn from(e: std::io::Error) -> EnvelopeError {
        EnvelopeError::Io(e)
    }
}

fn not_for_this_session() -> EnvelopeError {
    EnvelopeError::NotForThisSession
}

fn malformed(e: crate::Error) -> EnvelopeError {
    EnvelopeError::Malformed(e.to_string())
}

/// The bytes of an envelope's associated data: its version, its identifier
/// and the relay's pair.
fn aad(id: &[u8; ID], pair: crate::Pair) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + ID + 32);
    out.push(VERSION);
    out.extend_from_slice(id);
    out.extend_from_slice(&pair.bytes());
    out
}

/// Key material of `label` over the pair and the plaintext.
fn derived(
    exporter: &Exporter,
    label: &str,
    pair: crate::Pair,
    plaintext: &[u8],
    out: &mut [u8],
) -> Result<(), EnvelopeError> {
    let mut context = Vec::with_capacity(32 + plaintext.len());
    context.extend_from_slice(&pair.bytes());
    context.extend_from_slice(plaintext);
    exporter.expand(label, &context, out).map_err(malformed)
}

/// The padded body of a destination: its host's length in one byte, the
/// host, the port (big-endian), the service's length in one byte, the
/// service, then zero bytes.
fn encode(host: &str, port: u16, service: &str) -> Result<[u8; BODY], EnvelopeError> {
    let (host, service) = (host.as_bytes(), service.as_bytes());
    if host.is_empty() {
        return Err(EnvelopeError::Malformed("a destination with no host".into()));
    }
    let need = 1 + host.len() + 2 + 1 + service.len();
    if need > BODY || host.len() > u8::MAX as usize || service.len() > u8::MAX as usize {
        return Err(EnvelopeError::TooLong(need));
    }
    let mut body = [0u8; BODY];
    body[0] = host.len() as u8;
    body[1..1 + host.len()].copy_from_slice(host);
    let at = 1 + host.len();
    body[at..at + 2].copy_from_slice(&port.to_be_bytes());
    body[at + 2] = service.len() as u8;
    body[at + 3..at + 3 + service.len()].copy_from_slice(service);
    Ok(body)
}

/// The destination of a padded body, checked.
fn decode(body: &[u8; BODY]) -> Result<Destination, EnvelopeError> {
    let host_len = body[0] as usize;
    if host_len == 0 {
        return Err(EnvelopeError::Malformed("a destination with no host".into()));
    }
    let at = 1 + host_len;
    if at + 3 > BODY {
        return Err(EnvelopeError::Malformed("a host that runs past the body".into()));
    }
    let host = std::str::from_utf8(&body[1..at])
        .map_err(|_| EnvelopeError::Malformed("a host that is not UTF-8".into()))?
        .to_string();
    let port = u16::from_be_bytes([body[at], body[at + 1]]);
    let service_len = body[at + 2] as usize;
    let end = at + 3 + service_len;
    if end > BODY {
        return Err(EnvelopeError::Malformed("a service that runs past the body".into()));
    }
    let service = std::str::from_utf8(&body[at + 3..end])
        .map_err(|_| EnvelopeError::Malformed("a service that is not UTF-8".into()))?
        .to_string();
    if body[end..].iter().any(|b| *b != 0) {
        return Err(EnvelopeError::Malformed("a destination with bytes after its padding".into()));
    }
    Ok(Destination { host, port, service })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_encodes_and_decodes_in_one_bucket() {
        let body = encode("db.internal", 5432, "postgres").unwrap();
        let destination = decode(&body).unwrap();
        assert_eq!((destination.host(), destination.port(), destination.service()), ("db.internal", 5432, "postgres"));
        assert_eq!((destination.authority()), "db.internal:5432".to_string());
        assert!(encode("", 1, "").is_err(), "no host");
        assert!(matches!(encode(&"h".repeat(BODY), 1, ""), Err(EnvelopeError::TooLong(_))));
        let longest = decode(&encode(&"h".repeat(249), 65535, "svc").unwrap()).unwrap();
        assert_eq!(longest.port(), 65535);
    }

    #[test]
    fn a_body_with_bytes_after_its_padding_is_refused() {
        let mut body = encode("h", 1, "s").unwrap();
        body[BODY - 1] = 1;
        assert!(matches!(decode(&body), Err(EnvelopeError::Malformed(_))));
    }
}
