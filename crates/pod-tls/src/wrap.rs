//! A verified TLS handshake over a stream that is open already: a TCP
//! connection, or an SSH channel that reaches a hop inside TLS. Each road
//! takes the same configuration and the same words of failure.

use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use rustls::ClientConfig;
use rustls_pki_types::ServerName;

use crate::context::Context;
use crate::hint::{Words, handshake_error};
use crate::trust::Trust;

/// A verified TLS handshake for `server_name` over `stream` with `config`,
/// within `timeout`. A refusal has the words of
/// [`crate::handshake_failure`] and the error of the handshake as its cause.
pub async fn tls_over<S>(
    stream: S,
    server_name: &str,
    config: Arc<ClientConfig>,
    timeout: Duration,
    words: &Words,
) -> Result<tokio_rustls::client::TlsStream<S>, Failure>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let name = self::server_name(server_name)?;
    // The connector gets its config explicitly, never a process default.
    let connector = tokio_rustls::TlsConnector::from(config);
    tokio::time::timeout(timeout, connector.connect(name, stream))
        .await
        .map_err(|_| timed_out(timeout))?
        // rustls's text distinguishes an unknown issuer from a name mismatch,
        // and an unknown issuer gets what to do; the kind of a cut is kept in
        // the cause.
        .map_err(|e| handshake_error(e, words))
}

impl Context {
    /// [`tls_over`] with the configuration of `trust` in this context.
    pub async fn tls_over<S>(
        &self,
        stream: S,
        server_name: &str,
        trust: &Trust,
        timeout: Duration,
    ) -> Result<tokio_rustls::client::TlsStream<S>, Failure>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let config = self.config_for(trust)?;
        tls_over(stream, server_name, config, timeout, self.words()).await
    }
}

/// The name that TLS checks, refused before anything connects.
pub(crate) fn server_name(server_name: &str) -> Result<ServerName<'static>, Failure> {
    ServerName::try_from(server_name.to_string())
        .map_err(|e| Failure::new(Stage::Tls, Class::Usage, format!("{server_name:?} is not a valid server name: {e}")))
}

/// A handshake that did not end within `timeout`.
pub(crate) fn timed_out(timeout: Duration) -> Failure {
    Failure::new(Stage::Tls, Class::Timeout, format!("the TLS handshake timed out after {}s", timeout.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::pin::Pin;
    use std::task::{Context as Task, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    /// A far end of the tests: each write taken; a read ends as `read`
    /// says: an error of that kind, the end of the stream, or never.
    struct Peer {
        read: Option<Option<ErrorKind>>,
    }

    impl AsyncRead for Peer {
        fn poll_read(self: Pin<&mut Self>, _: &mut Task<'_>, _: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
            match self.read {
                Some(Some(kind)) => Poll::Ready(Err(kind.into())),
                Some(None) => Poll::Ready(Ok(())),
                // Silence: the bound of the handshake ends it.
                None => Poll::Pending,
            }
        }
    }

    impl AsyncWrite for Peer {
        fn poll_write(self: Pin<&mut Self>, _: &mut Task<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Task<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Task<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    async fn handshake(read: Option<Option<ErrorKind>>) -> Failure {
        let bound = Duration::from_millis(300);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let context = Context::new(provider, crate::Settings::new(Words::new("podssh")));
        match context.tls_over(Peer { read }, "relay.example", &Trust::Default, bound).await {
            Ok(_) => panic!("a handshake with no server completed"),
            Err(e) => e,
        }
    }

    fn kind(failure: &Failure) -> Option<ErrorKind> {
        std::error::Error::source(failure).and_then(|e| e.downcast_ref::<std::io::Error>()).map(|e| e.kind())
    }

    /// The handshake keeps the kind that ended it: a reset after the
    /// ClientHello, a close with nothing, and silence to the bound.
    #[tokio::test]
    async fn the_handshake_keeps_a_reset_a_close_and_a_timeout_apart() {
        let reset = handshake(Some(Some(ErrorKind::ConnectionReset))).await;
        assert_eq!((reset.stage, kind(&reset)), (Stage::Tls, Some(ErrorKind::ConnectionReset)), "{reset}");
        let closed = handshake(Some(None)).await;
        assert_eq!(kind(&closed), Some(ErrorKind::UnexpectedEof), "{closed}");
        let silent = handshake(None).await;
        assert_eq!((silent.class, kind(&silent)), (Class::Timeout, None), "{silent}");
        assert_eq!(silent.text(), "the TLS handshake timed out after 0s");
    }

    #[test]
    fn a_name_that_is_no_server_name_is_refused_before_a_connection() {
        let refused = server_name("not a name").expect_err("a name with spaces");
        assert_eq!((refused.stage, refused.class), (Stage::Tls, Class::Usage));
        assert!(refused.text().contains("is not a valid server name"), "{refused}");
    }
}
