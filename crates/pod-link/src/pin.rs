//! The parts that each carrier with the pin of an Ed25519 key shares
//! (`tcp`, `ws`, `h2`): the name of the ClientHello, the class of a
//! failure of the way out, a refused key in a failed handshake, the key
//! that the peer proved, and the dial through the way out.

use std::fmt;
use std::io;

use pod_core::{Class, Connected, Dial, Failure, Target};
use pod_identity::PublicKey;
use pod_tls::peer::{self, refusal};
use rustls::pki_types::ServerName;
use tokio::time::{Duration, Instant, timeout_at};

use crate::carrier::{CarrierError, CarrierErrorKind};

/// The name of the ClientHello: a DNS name goes as SNI, an address sends
/// none. No side checks it: the pin names the peer.
pub(crate) fn server_name(carrier: &'static str, target: &Target) -> Result<ServerName<'static>, CarrierError> {
    match target {
        Target::Addr(addr) => Ok(ServerName::IpAddress(addr.ip().into())),
        Target::Name { host, .. } => ServerName::try_from(host.clone()).map_err(|_| {
            CarrierError::new(carrier, CarrierErrorKind::Usage, format!("{host:?} is no DNS name and no address"))
        }),
    }
}

/// A dial of `target` through `way` within `limit`.
pub(crate) async fn dial(
    carrier: &'static str,
    way: &dyn Dial,
    target: &Target,
    limit: Duration,
) -> Result<Connected, CarrierError> {
    let label = way.label();
    let deadline = Instant::now() + limit;
    match timeout_at(deadline, way.dial(target, deadline)).await {
        Err(_) => Err(CarrierError::new(
            carrier,
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("connect to {target} through {label}: no answer in {limit:?}"),
        )),
        Ok(Err(failure)) => Err(of_failure(carrier, target, &label, failure)),
        Ok(Ok(connected)) => Ok(connected),
    }
}

/// A failure of the way out, as an error of the carrier with its class.
pub(crate) fn of_failure(carrier: &'static str, target: &Target, label: &str, failure: Failure) -> CarrierError {
    let kind = match failure.class {
        Class::Usage => CarrierErrorKind::Usage,
        Class::NotFound => CarrierErrorKind::NotFound,
        Class::Refused | Class::Policy | Class::Revoked => CarrierErrorKind::Refused,
        Class::Timeout => CarrierErrorKind::Io(io::ErrorKind::TimedOut),
        Class::Protocol => CarrierErrorKind::Io(io::ErrorKind::InvalidData),
        _ => CarrierErrorKind::Unavailable,
    };
    let text = format!("connect to {target} through {label}");
    CarrierError::with_source(carrier, kind, text, io::Error::other(failure))
}

/// A failed handshake: `Refused` with the key that the peer proved, when
/// the pin refused it.
pub(crate) fn tls_error(carrier: &'static str, text: String, e: io::Error) -> CarrierError {
    match refusal(&e) {
        Some(found) => CarrierError::with_source(carrier, CarrierErrorKind::Refused, format!("{text}: {found}"), e),
        None => CarrierError::io(carrier, text, e),
    }
}

/// The error of the wait for the first answer of the listener: an alert
/// of a refused certificate is a refusal of the key of this end.
pub(crate) fn answer_error(carrier: &'static str, target: &Target, own: &PublicKey, e: io::Error) -> CarrierError {
    use rustls::AlertDescription as A;
    let alert = match e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()) {
        Some(rustls::Error::AlertReceived(alert)) => Some(*alert),
        _ => None,
    };
    match alert {
        Some(
            alert @ (A::CertificateUnknown
            | A::BadCertificate
            | A::AccessDenied
            | A::UnsupportedCertificate
            | A::CertificateRequired),
        ) => CarrierError::with_source(
            carrier,
            CarrierErrorKind::Refused,
            format!("the listener at {target} refused the key of this end, {} ({alert:?})", own.fingerprint()),
            e,
        ),
        _ if e.kind() == io::ErrorKind::UnexpectedEof => {
            CarrierError::io(carrier, format!("the listener at {target} closed before it took this end"), e)
        }
        _ => tls_error(carrier, format!("the acceptance of the listener at {target}"), e),
    }
}

/// The ALPN of a handshake must be `want`.
pub(crate) fn check_alpn(
    carrier: &'static str,
    alpn: Option<&[u8]>,
    want: &[u8],
    peer: &dyn fmt::Display,
) -> Result<(), CarrierError> {
    match alpn {
        Some(got) if got == want => Ok(()),
        _ => Err(CarrierError::new(
            carrier,
            CarrierErrorKind::Io(io::ErrorKind::InvalidData),
            format!("the peer at {peer} took no ALPN {}", String::from_utf8_lossy(want)),
        )),
    }
}

/// The key that the peer proved. The verifier of `pod_tls::peer` let the
/// handshake end only with one.
pub(crate) fn proved(
    carrier: &'static str,
    certificates: Option<&[rustls::pki_types::CertificateDer<'_>]>,
    peer: &dyn fmt::Display,
) -> Result<PublicKey, CarrierError> {
    peer::peer_key(certificates).ok_or_else(|| {
        CarrierError::new(
            carrier,
            CarrierErrorKind::Io(io::ErrorKind::InvalidData),
            format!("the peer at {peer} proved no Ed25519 key"),
        )
    })
}

/// The provider makes TLS 1.3 and verifies Ed25519, and a TCP socket binds
/// on the loopback: the host allows the family.
pub(crate) fn available(carrier: &'static str, provider: &rustls::crypto::CryptoProvider) -> Result<(), CarrierError> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    peer::supports(provider).map_err(|why| CarrierError::new(carrier, CarrierErrorKind::Unavailable, why))?;
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .or_else(|_| std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)))
        .map(drop)
        .map_err(|e| {
            CarrierError::new(carrier, CarrierErrorKind::Unavailable, format!("TCP sockets on the loopback: {e}"))
        })
}
