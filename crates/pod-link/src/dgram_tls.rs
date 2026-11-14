//! The handshake that the carriers of datagrams which lose frames share
//! (`dns`, `icmp`): the selective-repeat ARQ ([`crate::arq::ArqStream`])
//! makes one reliable byte stream of the carrier's datagrams, and the TLS
//! 1.3 of the tcp carrier runs above it. Each side pins the Ed25519 key of
//! the other (`pod_tls::peer`), and TLS seals each byte, so a datagram of a
//! stranger can end a link but not change it. After its handshake the
//! listener writes the byte of acceptance, as the tcp carrier does (calls
//! C274, C275).

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Target;
use pod_identity::{Identity, KeyName, PublicKey};
use pod_tls::peer::{self, Keys};
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::time::timeout;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::arq::{ArqSettings, ArqStream};
use crate::carrier::{BoxIo, CarrierError, CarrierErrorKind};
use crate::datagram::Datagrams;
use crate::pin;

/// The byte that a listener writes when it took the key of the dialer.
pub(crate) const ACCEPTED: u8 = 1;

fn err(name: &'static str, kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(name, kind, text)
}

/// The ARQ over `datagrams`, within its limits. TLS above it seals each
/// record with a nonce of its own, so the sequence of the ARQ is no nonce:
/// the ARQ wraps with no rekey, and no direction stops at the wrap (call
/// C290).
fn stream<D: Datagrams>(name: &'static str, datagrams: D, arq: ArqSettings) -> Result<ArqStream, CarrierError> {
    let stream = ArqStream::new(datagrams, arq).map_err(|e| err(name, CarrierErrorKind::Usage, format!("ARQ: {e}")))?;
    stream.wrap_freely();
    Ok(stream)
}

/// The TLS of a listener of a datagram carrier: the provider, the identity,
/// the client keys and the ALPN.
pub(crate) fn acceptor(
    name: &'static str,
    provider: &Arc<CryptoProvider>,
    identity: &Arc<Identity>,
    clients: &Keys,
    alpn: &'static [u8],
) -> Result<TlsAcceptor, CarrierError> {
    let config = peer::server_config(provider.clone(), identity.clone(), clients.clone(), &[alpn])
        .map_err(|e| err(name, CarrierErrorKind::Unavailable, e.to_string()))?;
    Ok(TlsAcceptor::from(config))
}

/// The dial's handshake over the datagrams of one peer: the ARQ, TLS with
/// the pin of `peer`, the ALPN, and the byte of acceptance. `target` names
/// the peer (and its SNI when it is a name). Within `within`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn connect<D: Datagrams>(
    name: &'static str,
    datagrams: D,
    provider: &Arc<CryptoProvider>,
    identity: &Arc<Identity>,
    peer_key: &KeyName,
    alpn: &'static [u8],
    arq: ArqSettings,
    target: &Target,
    within: Duration,
) -> Result<(BoxIo, PublicKey), CarrierError> {
    let config = peer::client_config(provider.clone(), identity.clone(), peer_key.clone(), &[alpn])
        .map_err(|e| err(name, CarrierErrorKind::Unavailable, e.to_string()))?;
    let server = pin::server_name(name, target)?;
    let stream = stream(name, datagrams, arq)?;
    let own = identity.public();
    let shake = async {
        let mut tls = TlsConnector::from(config)
            .connect(server, stream)
            .await
            .map_err(|e| pin::tls_error(name, format!("the TLS handshake with {target}"), e))?;
        let conn = tls.get_ref().1;
        pin::check_alpn(name, conn.alpn_protocol(), alpn, target)?;
        let key = pin::proved(name, conn.peer_certificates(), target)?;
        let mut word = [0u8];
        match tls.read_exact(&mut word).await {
            Ok(_) if word[0] == ACCEPTED => Ok((Box::new(tls) as BoxIo, key)),
            Ok(_) => Err(err(
                name,
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                format!("the listener at {target} sent {} where its acceptance goes", word[0]),
            )),
            Err(e) => Err(pin::answer_error(name, target, &own, e)),
        }
    };
    timeout(within, shake).await.unwrap_or_else(|_| {
        Err(err(
            name,
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the handshake with {target} took more than {within:?}"),
        ))
    })
}

/// The listener's handshake over the datagrams of one peer: the ARQ, TLS
/// that takes a pinned client key, the ALPN, and the byte of acceptance.
pub(crate) async fn accept<D: Datagrams>(
    name: &'static str,
    datagrams: D,
    acceptor: TlsAcceptor,
    alpn: &'static [u8],
    arq: ArqSettings,
    from: &(dyn fmt::Display + Sync),
    within: Duration,
) -> Result<(BoxIo, PublicKey), CarrierError> {
    let stream = stream(name, datagrams, arq)?;
    let run = async {
        let mut tls = acceptor
            .accept(stream)
            .await
            .map_err(|e| pin::tls_error(name, format!("the TLS handshake of {from}"), e))?;
        let conn = tls.get_ref().1;
        pin::check_alpn(name, conn.alpn_protocol(), alpn, from)?;
        let key = pin::proved(name, conn.peer_certificates(), from)?;
        let said = async {
            tls.write_all(&[ACCEPTED]).await?;
            tls.flush().await
        };
        said.await.map_err(|e| CarrierError::io(name, format!("the acceptance of {from}"), e))?;
        Ok((Box::new(tls) as BoxIo, key))
    };
    timeout(within, run).await.unwrap_or_else(|_| {
        Err(err(
            name,
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the handshake of {from} took more than {within:?}"),
        ))
    })
}
