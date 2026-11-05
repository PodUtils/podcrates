//! Why a trust, a configuration or the pair of a server cannot be used. A
//! handshake that failed is a [`pod_core::Failure`] at
//! [`pod_core::Stage::Tls`]; each error here becomes one with the class
//! [`pod_core::Class::Config`].

use std::fmt;

use pod_core::{Class, Failure, Stage};

/// A trust, a configuration or the pair of a server that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The trust bundle could not be used. The path is always in the
    /// words: a deployment that cannot find its trust store cannot be
    /// diagnosed from "TLS failed".
    Bundle { path: String, why: String },
    /// The `ClientConfig` could not be made, or a caller's configuration
    /// cannot be used.
    Config(String),
    /// The certificate or the key of a server cannot be used ([`crate::server`]).
    Server(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Bundle { path, why } => write!(f, "CA bundle {path}: {why}"),
            Error::Config(why) => write!(f, "TLS configuration: {why}"),
            Error::Server(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::new(Stage::Tls, Class::Config, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_is_a_failure_of_the_configuration_at_the_tls_stage() {
        let bundle = Error::Bundle { path: "/etc/ca.pem".into(), why: "no such file".into() };
        assert_eq!(bundle.to_string(), "CA bundle /etc/ca.pem: no such file");
        let failure = Failure::from(bundle);
        assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Config));
        assert_eq!(failure.text(), "CA bundle /etc/ca.pem: no such file");
        assert_eq!(Error::Config("no roots".into()).to_string(), "TLS configuration: no roots");
        assert_eq!(Error::Server("the key is encrypted".into()).to_string(), "the key is encrypted");
    }
}
