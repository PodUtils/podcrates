//! The bare form of the DNS carrier ([`crate::dns::Dns::bare`]): the ARQ
//! with no TLS above it, for a caller that authenticates and seals its own
//! stream (podssh's Noise layer). Its first record is a hello of its own, not
//! a ClientHello, so a bare end and an end with TLS never pair: a listener
//! with TLS takes no slot for a bare hello, and a bare listener takes none
//! for a ClientHello (`dns::hello`). The hello names the ALPN of the link;
//! the listener answers it with the byte of acceptance of the tcp carrier.
//!
//! The bare form pins no key and seals no byte: a stranger on the path (a
//! resolver, a sender of a query with the session id) can read, change, add
//! and drop the bytes of the stream. The ARQ wraps its sequence freely, as
//! under TLS: the seal of the caller must have nonces of its own.

use std::fmt;
use std::io;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::time::timeout;

use super::{NAME, err};
use crate::arq::ArqSettings;
use crate::carrier::{BoxIo, CarrierError, CarrierErrorKind};
use crate::datagram::Datagrams;
use crate::dgram_tls::{self, ACCEPTED};

/// The first bytes of the hello of a bare dialer. The first byte is no
/// content type of TLS (RFC 8446, section 5.1), so a listener with TLS reads
/// no ClientHello in it.
pub(super) const MAGIC: &[u8] = b"PODBARE1";

/// The hello of a bare dialer for `alpn`: [`MAGIC`], the length of the ALPN
/// in one byte, the ALPN.
pub(super) fn hello(alpn: &[u8]) -> Vec<u8> {
    let len = u8::try_from(alpn.len()).unwrap_or(u8::MAX);
    [MAGIC, &[len], &alpn[..usize::from(len)]].concat()
}

/// The handshake time limit is over.
fn late(what: String, within: Duration) -> CarrierError {
    let why = format!("{what} took more than {within:?} (an end with TLS sends no answer to a bare end)");
    err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), why)
}

/// The bare dial over the datagrams of one peer: the ARQ, the hello for
/// `alpn`, and the byte of acceptance, within `within`.
pub(super) async fn connect<D: Datagrams>(
    datagrams: D,
    alpn: &'static [u8],
    arq: ArqSettings,
    target: &(dyn fmt::Display + Sync),
    within: Duration,
) -> Result<BoxIo, CarrierError> {
    let mut stream = dgram_tls::stream(NAME, datagrams, arq)?;
    let shake = async {
        stream.write_all(&hello(alpn)).await?;
        stream.flush().await?;
        let mut word = [0u8];
        stream.read_exact(&mut word).await?;
        Ok::<u8, io::Error>(word[0])
    };
    match timeout(within, shake).await {
        Err(_) => Err(late(format!("the bare handshake with {target}"), within)),
        Ok(Err(e)) => Err(CarrierError::io(NAME, format!("the bare handshake with {target}"), e)),
        Ok(Ok(ACCEPTED)) => Ok(Box::new(stream)),
        Ok(Ok(other)) => Err(err(
            CarrierErrorKind::Io(io::ErrorKind::InvalidData),
            format!("the listener at {target} sent {other} where its bare acceptance goes"),
        )),
    }
}

/// The bare accept over the datagrams of one peer: the ARQ, the hello of the
/// dialer, which must offer `alpn`, and the byte of acceptance, within
/// `within`. A first record of another form (a ClientHello) is refused.
pub(super) async fn accept<D: Datagrams>(
    datagrams: D,
    alpn: &'static [u8],
    arq: ArqSettings,
    from: &(dyn fmt::Display + Sync),
    within: Duration,
) -> Result<BoxIo, CarrierError> {
    let mut stream = dgram_tls::stream(NAME, datagrams, arq)?;
    let want = hello(alpn);
    let shake = async {
        let mut got = vec![0u8; want.len()];
        stream.read_exact(&mut got).await?;
        if got != want {
            return Ok(false);
        }
        stream.write_all(&[ACCEPTED]).await?;
        stream.flush().await?;
        Ok::<bool, io::Error>(true)
    };
    match timeout(within, shake).await {
        Err(_) => Err(late(format!("the bare handshake of {from}"), within)),
        Ok(Err(e)) => Err(CarrierError::io(NAME, format!("the bare handshake of {from}"), e)),
        Ok(Ok(true)) => Ok(Box::new(stream)),
        Ok(Ok(false)) => Err(err(
            CarrierErrorKind::Refused,
            format!(
                "{from} sent no bare hello of {:?}: an end with TLS, or another ALPN",
                String::from_utf8_lossy(alpn)
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hello_opens_no_tls_record_and_names_its_alpn() {
        let hello = hello(b"podlink/1");
        assert_eq!(hello, b"PODBARE1\x09podlink/1");
        assert_ne!(hello[0], 0x16, "no TLS handshake record");
    }
}
