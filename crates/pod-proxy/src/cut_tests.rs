//! The tests of the cuts: each kind of I/O, a handshake's refusal and its
//! cut, and the cut of each failure of a road.

use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_tls::{Trust, Words};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::*;

#[test]
fn each_kind_is_its_cut() {
    assert_eq!(Cut::of(ErrorKind::ConnectionReset), Cut::Reset);
    assert_eq!(Cut::of(ErrorKind::ConnectionAborted), Cut::Reset);
    assert_eq!(Cut::of(ErrorKind::ConnectionRefused), Cut::Refused);
    assert_eq!(Cut::of(ErrorKind::TimedOut), Cut::TimedOut);
    assert_eq!(Cut::of(ErrorKind::UnexpectedEof), Cut::Closed);
    assert_eq!(Cut::of(ErrorKind::PermissionDenied), Cut::Other);
    // A reset in Windows' words is a reset in its kind (WSAECONNRESET).
    #[cfg(windows)]
    assert_eq!(Cut::of(std::io::Error::from_raw_os_error(10054).kind()), Cut::Reset);
}

/// A certificate's refusal has no cut; a reset in the handshake has one.
/// The failures of a road: a handshake's, a bound's, a dial's, and a
/// refusal's, which has none.
#[test]
fn a_handshake_s_refusal_has_no_cut() {
    let words = Words::new("podssh");
    let refused = std::io::Error::new(ErrorKind::InvalidData, "invalid peer certificate: UnknownIssuer");
    assert_eq!(TlsFailure::of(&refused, &words).cut, None);
    let reset = std::io::Error::from(ErrorKind::ConnectionReset);
    assert_eq!(TlsFailure::of(&reset, &words).cut, Some(Cut::Reset));
    assert_eq!(TlsFailure::from("words").to_string(), "words");
    let tls = pod_tls::handshake_error(std::io::Error::from(ErrorKind::ConnectionReset), &words);
    assert_eq!(Cut::of_failure(&tls), Some(Cut::Reset));
    let late = Failure::new(Stage::Tls, Class::Timeout, "the TLS handshake timed out after 10s");
    assert_eq!(Cut::of_failure(&late), Some(Cut::TimedOut));
    let dial = DialError::Connect { target: "t:443".into(), detail: "d".into(), cut: Some(Cut::Refused) };
    assert_eq!(Cut::of_failure(&Failure::from(dial)), Some(Cut::Refused));
    let status = Failure::new(Stage::WebSocket, Class::Refused, "the relay refused the session: HTTP 403");
    assert_eq!(Cut::of_failure(&status), None);
}

/// A far end of the tests: each write taken; a read ends as `read`
/// says: an error of that kind, the end of the stream, or never.
struct Peer {
    read: Option<Option<ErrorKind>>,
}

impl AsyncRead for Peer {
    fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.read {
            Some(Some(kind)) => Poll::Ready(Err(kind.into())),
            Some(None) => Poll::Ready(Ok(())),
            // Silence: the bound of the handshake ends it.
            None => Poll::Pending,
        }
    }
}

impl AsyncWrite for Peer {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

async fn handshake(read: Option<Option<ErrorKind>>) -> Failure {
    let bound = Duration::from_millis(300);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = pod_tls::Context::new(provider, pod_tls::Settings::new(Words::new("podssh")));
    match context.tls_over(Peer { read }, "relay.example", &Trust::Default, bound).await {
        Ok(_) => panic!("a handshake with no server completed"),
        Err(e) => e,
    }
}

/// The handshake keeps the kind that ended it: a reset after the
/// ClientHello, a close with nothing, and silence to the bound.
#[tokio::test]
async fn the_handshake_keeps_a_reset_a_close_and_a_timeout_apart() {
    let reset = handshake(Some(Some(ErrorKind::ConnectionReset))).await;
    assert!(reset.stage == Stage::Tls && reset.class != Class::Timeout, "{reset:?}");
    assert_eq!(Cut::of_failure(&reset), Some(Cut::Reset), "{reset}");
    let closed = handshake(Some(None)).await;
    assert_eq!(Cut::of_failure(&closed), Some(Cut::Closed), "{closed}");
    let silent = handshake(None).await;
    assert_eq!(silent.class, Class::Timeout, "{silent:?}");
    assert_eq!(Cut::of_failure(&silent), Some(Cut::TimedOut));
}
