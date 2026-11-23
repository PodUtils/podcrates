//! The failures of the crate (`docs/design.md`, section 1, rule 9): each is
//! a [`pod_core::Failure`] with its stage (the part of the path that broke,
//! podnet's hop) and its class (what a caller can do, podnet's exit code).
//! A context keeps the stage and the class of its cause, and its text holds
//! the text of each cause.

use std::error::Error;
use std::fmt;

pub use pod_core::Failure;
use pod_core::{Class, Stage};

/// A result with a [`Failure`].
pub type Result<T, E = Failure> = std::result::Result<T, E>;

/// A failure with no cause.
pub fn new(stage: Stage, class: Class, text: impl Into<String>) -> Failure {
    Failure::new(stage, class, text)
}

/// A failure at `stage` of class `class` whose cause is `cause`. The text is
/// `text`, a colon and the text of the cause.
pub fn caused(
    stage: Stage,
    class: Class,
    text: impl fmt::Display,
    cause: impl Error + Send + Sync + 'static,
) -> Failure {
    let words = format!("{text}: {cause}");
    Failure::caused(stage, class, words, cause)
}

/// Return a [`Failure`] from the function: the stage, the class and the
/// words of `format!`.
macro_rules! bail {
    ($stage:expr, $class:expr, $($arg:tt)+) => {
        return Err($crate::fail::new($stage, $class, format!($($arg)+)))
    };
}
pub(crate) use bail;

/// The context of a failure: a text before the text of the failure. The
/// stage and the class stay.
pub trait Context<T> {
    fn context(self, text: impl fmt::Display) -> Result<T>;
    fn with_context<D: fmt::Display>(self, text: impl FnOnce() -> D) -> Result<T>;
}

impl<T> Context<T> for Result<T> {
    fn context(self, text: impl fmt::Display) -> Result<T> {
        self.map_err(|e| wrap(e, text))
    }

    fn with_context<D: fmt::Display>(self, text: impl FnOnce() -> D) -> Result<T> {
        self.map_err(|e| wrap(e, text()))
    }
}

/// `failure` with the text `text` before its own, once: a failure that
/// starts with `text` (the request that `pod-proxy` names) keeps its words.
pub fn wrap(failure: Failure, text: impl fmt::Display) -> Failure {
    let (stage, class) = (failure.stage, failure.class);
    let words = pod_core::failure::lead(&text.to_string(), &failure.chain());
    Failure::caused(stage, class, words, failure)
}

/// A stage and a class for an error of another crate.
pub trait OrFail<T> {
    fn or_fail(self, stage: Stage, class: Class, text: impl fmt::Display) -> Result<T>;
}

impl<T, E: Error + Send + Sync + 'static> OrFail<T> for std::result::Result<T, E> {
    fn or_fail(self, stage: Stage, class: Class, text: impl fmt::Display) -> Result<T> {
        self.map_err(|e| caused(stage, class, text, e))
    }
}

impl<T> OrFail<T> for Option<T> {
    fn or_fail(self, stage: Stage, class: Class, text: impl fmt::Display) -> Result<T> {
        self.ok_or_else(|| new(stage, class, text.to_string()))
    }
}

/// The stage of an error of I/O; its class comes from its kind. The text is
/// the step, and the error is the cause: the chain says it once.
pub trait IoFail<T> {
    fn io_fail(self, stage: Stage, text: impl fmt::Display) -> Result<T>;
}

impl<T> IoFail<T> for std::io::Result<T> {
    fn io_fail(self, stage: Stage, text: impl fmt::Display) -> Result<T> {
        self.map_err(|e| io_step(stage, text, e))
    }
}

/// The failure of the step `text` at `stage` that the error of I/O `e`
/// ended: the step as the text and `e` as the cause, as
/// [`Failure::caused`] says.
pub fn io_step(stage: Stage, text: impl fmt::Display, e: std::io::Error) -> Failure {
    Failure::caused(stage, Class::of_io(e.kind()), text.to_string(), e)
}

/// The first error of type `E` in the chain of `failure`, the failure
/// itself included.
pub fn find<E: Error + 'static>(failure: &Failure) -> Option<&E> {
    let mut next: Option<&(dyn Error + 'static)> = Some(failure);
    while let Some(e) = next {
        if let Some(found) = e.downcast_ref::<E>() {
            return Some(found);
        }
        next = e.source();
    }
    None
}

/// The stage of `failure`.
pub fn stage_of(failure: &Failure) -> Option<Stage> {
    Some(failure.stage)
}

/// The class of `failure`.
pub fn class_of(failure: &Failure) -> Option<Class> {
    Some(failure.class)
}

/// The refusal of a registration by the edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    /// The name of the client in the text: "NAME speaks cloudflared ...".
    pub client: String,
    pub cause: String,
    pub should_retry: bool,
    pub retry_after: std::time::Duration,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the edge refused the registration: {} (retry: {}, after {} s; {} speaks {})",
            self.cause,
            if self.should_retry { "yes" } else { "no" },
            self.retry_after.as_secs(),
            self.client,
            crate::cloudflare::speaks()
        )
    }
}

impl Error for Refused {}

impl From<Refused> for Failure {
    /// A refusal with no retry is final. A refusal with a retry is
    /// temporary.
    fn from(refused: Refused) -> Failure {
        let class = if refused.should_retry { Class::TempFail } else { Class::Refused };
        Failure::caused(Stage::EdgeRegister, class, refused.to_string(), refused)
    }
}

/// The text of an I/O error of a TLS handshake: an alert of the peer is
/// named; a reset or an end of the stream is said as such.
pub fn tls_text(err: &std::io::Error) -> String {
    if let Some(inner) = err.get_ref()
        && let Some(rustls::Error::AlertReceived(alert)) = inner.downcast_ref::<rustls::Error>()
    {
        return format!("TLS alert from the peer: {alert:?}");
    }
    match err.kind() {
        std::io::ErrorKind::ConnectionReset => {
            format!("the connection was reset during the TLS handshake ({err})")
        }
        std::io::ErrorKind::UnexpectedEof => "the connection was closed during the TLS handshake".to_string(),
        _ => err.to_string(),
    }
}

/// The stage of an I/O error of a TLS handshake: an error of rustls is
/// TLS; a reset or a refusal is the network.
pub fn tls_stage(err: &std::io::Error) -> Stage {
    use std::io::ErrorKind::*;
    if err.get_ref().is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some()) {
        return Stage::Tls;
    }
    match err.kind() {
        ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof | TimedOut | ConnectionRefused => {
            Stage::Network
        }
        _ => Stage::Tls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_in_the_tls_handshake_is_the_network() {
        use std::io::{Error, ErrorKind};
        let reset = Error::from(ErrorKind::ConnectionReset);
        assert_eq!(tls_stage(&reset), Stage::Network);
        assert!(tls_text(&reset).contains("reset during the TLS handshake"));
        assert_eq!(tls_stage(&Error::from(ErrorKind::UnexpectedEof)), Stage::Network);
        let cert = Error::new(
            ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
        );
        assert_eq!(tls_stage(&cert), Stage::Tls);
    }

    #[test]
    fn the_innermost_stage_is_kept_through_context() {
        let err = Err::<(), _>(new(Stage::Proxy, Class::Refused, "403 not on the egress allowlist"))
            .context("POST https://api.trycloudflare.com/tunnel")
            .unwrap_err();
        assert_eq!(stage_of(&err), Some(Stage::Proxy));
        assert_eq!(format!("{err:#}"), "POST https://api.trycloudflare.com/tunnel: 403 not on the egress allowlist");
        assert_eq!(err.chain(), err.to_string());
    }

    #[test]
    fn a_class_survives_context() {
        let err = new(Stage::Host, Class::Config, "bad edge list");
        let err = Err::<(), _>(err).context("starting the tunnel").unwrap_err();
        assert_eq!(class_of(&err), Some(Class::Config));
        assert_eq!(format!("{err:#}"), "starting the tunnel: bad edge list");
    }

    /// A typed cause is found in the chain.
    #[test]
    fn a_refusal_is_found_through_context() {
        let refused = Refused {
            client: "pod-cloudflare".into(),
            cause: "Unauthorized".into(),
            should_retry: false,
            retry_after: Default::default(),
        };
        let err = Err::<(), _>(Failure::from(refused.clone())).context("registering").unwrap_err();
        assert_eq!(find::<Refused>(&err), Some(&refused));
        assert_eq!((err.stage, err.class), (Stage::EdgeRegister, Class::Refused));
    }
}
