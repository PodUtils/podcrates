//! The queries of a dialer of the DNS carrier over DNS over HTTPS (RFC
//! 8484, T-1168), for a host whose only way out is an HTTP proxy with no
//! UDP: each query goes as a GET of the wire form to one resolver, over one
//! connection through the way out of the dial ([`pod_dns::Pipe`]). Each
//! datagram up is one query, and the TXT records of each answer are the
//! datagrams down; the listener behind the resolver is the same. A `429` of
//! the resolver pauses the start of each query for its `Retry-After`
//! ([`DOH_PAUSE`] with none), at most [`DnsOverHttps::pause_max`]; the
//! query of the `429` is a lost datagram, and the ARQ sends it again.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use pod_core::Dial;
use pod_dns::{Doh, Pipe};
use pod_tls::Context;
use tokio::time::Instant;

/// The pause after a `429` with no `Retry-After`.
pub const DOH_PAUSE: Duration = Duration::from_secs(1);

/// The longest pause after a `429` by default: less than the handshake time
/// limit of the carrier (20 s) and than the time in which the ARQ gives up.
pub const DOH_PAUSE_MAX: Duration = Duration::from_secs(10);

/// The status of an answer that asks for fewer requests (RFC 6585).
const TOO_MANY: u16 = 429;

/// The DNS over HTTPS of a dialer ([`crate::dns::Dns::over_https`]): the
/// resolver, the settings of `pod-dns` that its connection takes (the trust,
/// the user agent, the time of a connection), the TLS of the binary, and the
/// longest pause at a `429`.
#[derive(Clone)]
pub struct DnsOverHttps {
    /// The resolver: its address and its name, its port, the path of its
    /// wire form (it must have one), and [`Doh::http2`]. The connection
    /// offers `h2` and `http/1.1` (`h2` alone with [`Doh::http2`]), and the
    /// resolver picks. Over HTTP/2 the queries share the connection at the
    /// same time; over HTTP/1.1 a query waits for the answer of the one
    /// before it, so a poll that the listener holds
    /// ([`crate::dns::DnsSettings::hold`]) delays the next query. The public
    /// resolvers pick HTTP/2.
    pub resolver: Doh,
    /// The trust, the user agent and the time of a connection (`connect`).
    pub settings: pod_dns::Settings,
    /// The TLS of the binary.
    pub context: Arc<Context>,
    /// The longest pause at a `429`, whatever its `Retry-After` says. By
    /// default [`DOH_PAUSE_MAX`].
    pub pause_max: Duration,
}

impl DnsOverHttps {
    /// The queries to `resolver` with the TLS of `context`, the default
    /// settings of `pod-dns` and [`DOH_PAUSE_MAX`].
    pub fn new(resolver: Doh, context: Arc<Context>) -> DnsOverHttps {
        DnsOverHttps { resolver, settings: pod_dns::Settings::default(), context, pause_max: DOH_PAUSE_MAX }
    }
}

impl fmt::Debug for DnsOverHttps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DnsOverHttps")
            .field("resolver", &self.resolver)
            .field("pause_max", &self.pause_max)
            .finish_non_exhaustive()
    }
}

/// The queries of one dial: the connection, and the end of a pause.
pub(super) struct Https {
    pipe: Pipe,
    pause_max: Duration,
    until: Mutex<Option<Instant>>,
}

impl Https {
    /// The queries of `doh` through `dial`.
    pub(super) fn new(doh: &DnsOverHttps, dial: Arc<dyn Dial>) -> Result<Https, String> {
        let pipe = Pipe::new(doh.resolver.clone(), doh.settings.clone(), doh.context.clone(), dial)?;
        Ok(Https { pipe, pause_max: doh.pause_max, until: Mutex::new(None) })
    }

    /// Wait for the end of a pause, when one runs.
    pub(super) async fn paused(&self) {
        loop {
            let until = *self.until.lock().unwrap_or_else(PoisonError::into_inner);
            match until {
                Some(until) if until > Instant::now() => tokio::time::sleep_until(until).await,
                _ => return,
            }
        }
    }

    /// One query `message`, and the DNS message of its answer. A failure, a
    /// `429` and each other status give none: the datagram is lost.
    pub(super) async fn ask(&self, message: &[u8]) -> Option<Vec<u8>> {
        let response = match self.pipe.ask(message).await {
            Ok(response) => response,
            Err(missed) => {
                tracing::debug!(host = %self.pipe.host(), why = %missed, "a query of a dns dialer over HTTPS failed");
                return None;
            }
        };
        match response.status {
            200 => Some(response.body),
            TOO_MANY => {
                let said = response.headers.iter().find(|(name, _)| name == "retry-after");
                let wait = said.and_then(|(_, value)| pod_core::clock::retry_after(value, SystemTime::now()));
                let wait = wait.unwrap_or(DOH_PAUSE).min(self.pause_max);
                let until = Instant::now() + wait;
                let mut pause = self.until.lock().unwrap_or_else(PoisonError::into_inner);
                *pause = Some(pause.map_or(until, |was| was.max(until)));
                tracing::debug!(host = %self.pipe.host(), ?wait, "the resolver of a dns dialer asked for fewer queries");
                None
            }
            status => {
                tracing::debug!(host = %self.pipe.host(), status, "a query of a dns dialer over HTTPS got no answer");
                None
            }
        }
    }
}
