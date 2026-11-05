//! Why a connection could not be opened: a [`DialError`], which names what
//! failed and the address or the proxy, and never a credential. Each one
//! becomes a [`pod_core::Failure`] with its stage and its class, and stays
//! its cause, so that a caller reads the variant and its cut back.

use std::time::Duration;

use pod_core::{Class, Failure, Stage};

use crate::cut::Cut;

/// Why a connection could not be opened. Every message names what failed and
/// the address or proxy involved, and never a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialError {
    /// The target or proxy name is not usable (empty, or contains whitespace
    /// or control characters that would break the request).
    InvalidTarget(String),
    /// The proxy settings in the environment could not be understood.
    BadProxy(String),
    /// The name did not resolve (direct connections only).
    Resolve { host: String, detail: String },
    /// Every address failed; the cut of the last one, by its kind.
    Connect { target: String, detail: String, cut: Option<Cut> },
    /// A step did not finish in time: the stage of the step (the proxy, the
    /// connection, the resolution).
    Timeout { step: String, after: Duration, stage: Stage },
    /// The proxy itself could not be reached.
    ProxyUnreachable { proxy: String, detail: String },
    /// The proxy answered CONNECT with a status other than 2xx; a SOCKS5
    /// proxy's refusal carries the HTTP status of the same case (403 for its
    /// rules, 407 for its authentication, 502 for the rest).
    ProxyRefused { proxy: String, target: String, status: u16, reason: String },
    /// The proxy did not answer as its protocol says.
    ProxyProtocol { proxy: String, detail: String },
}

impl std::fmt::Display for DialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialError::InvalidTarget(why) => write!(f, "invalid destination: {why}"),
            DialError::BadProxy(why) => write!(f, "the proxy setting is not usable: {why}"),
            DialError::Resolve { host, detail } => write!(f, "could not resolve {host}: {detail}"),
            DialError::Connect { target, detail, .. } => write!(f, "could not connect to {target}: {detail}"),
            DialError::Timeout { step, after, .. } => write!(f, "{step} timed out after {}s", after.as_secs()),
            DialError::ProxyUnreachable { proxy, detail } => {
                write!(f, "could not reach the proxy {proxy}: {detail}")
            }
            DialError::ProxyRefused { proxy, target, status, reason } => {
                write!(f, "the proxy {proxy} refused CONNECT {target}: {status} {reason}")
            }
            DialError::ProxyProtocol { proxy, detail } => {
                write!(f, "the proxy {proxy} did not answer as its protocol says: {detail}")
            }
        }
    }
}

impl std::error::Error for DialError {}

impl DialError {
    /// The stage of the path that broke, and what a caller can do about it.
    pub fn stage_and_class(&self) -> (Stage, Class) {
        match self {
            DialError::InvalidTarget(_) => (Stage::Connect, Class::Usage),
            DialError::BadProxy(_) => (Stage::Proxy, Class::Config),
            DialError::Resolve { .. } => (Stage::Dns, Class::Unavailable),
            DialError::Connect { cut: Some(Cut::TimedOut), .. } => (Stage::Connect, Class::Timeout),
            DialError::Connect { .. } => (Stage::Connect, Class::Unavailable),
            DialError::Timeout { stage, .. } => (*stage, Class::Timeout),
            DialError::ProxyUnreachable { .. } => (Stage::Proxy, Class::Unavailable),
            // The far side of the proxy, as a 502: another host may get through.
            DialError::ProxyRefused { status: 500..=599, .. } => (Stage::Proxy, Class::Unavailable),
            // Its authentication (407) or its rules (403, 405).
            DialError::ProxyRefused { .. } => (Stage::Proxy, Class::Refused),
            DialError::ProxyProtocol { .. } => (Stage::Proxy, Class::Protocol),
        }
    }
}

/// The failure of a dial, with the error as its cause: [`Cut::of_failure`]
/// and a downcast read it back.
impl From<DialError> for Failure {
    fn from(e: DialError) -> Failure {
        let (stage, class) = e.stage_and_class();
        Failure::caused(stage, class, e.to_string(), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_has_its_stage_and_class_and_stays_the_cause() {
        let refused = DialError::ProxyRefused {
            proxy: "proxy.example:3128".into(),
            target: "relay.example:443".into(),
            status: 407,
            reason: "Proxy Authentication Required".into(),
        };
        let failure = Failure::from(refused.clone());
        assert_eq!((failure.stage, failure.class), (Stage::Proxy, Class::Refused));
        assert_eq!(
            failure.text(),
            "the proxy proxy.example:3128 refused CONNECT relay.example:443: 407 Proxy Authentication Required"
        );
        let cause = std::error::Error::source(&failure).and_then(|e| e.downcast_ref::<DialError>());
        assert_eq!(cause, Some(&refused));
        let gateway =
            DialError::ProxyRefused { proxy: "p:1".into(), target: "t:2".into(), status: 502, reason: "".into() };
        assert_eq!(gateway.stage_and_class(), (Stage::Proxy, Class::Unavailable));
        let late =
            DialError::Timeout { step: "resolving relay.example".into(), after: Duration::ZERO, stage: Stage::Dns };
        assert_eq!(late.stage_and_class(), (Stage::Dns, Class::Timeout));
        let silent = DialError::Connect { target: "t:2".into(), detail: "d".into(), cut: Some(Cut::TimedOut) };
        assert_eq!(silent.stage_and_class(), (Stage::Connect, Class::Timeout));
        assert_eq!(DialError::InvalidTarget("x".into()).stage_and_class(), (Stage::Connect, Class::Usage));
        assert_eq!(DialError::BadProxy("x".into()).stage_and_class(), (Stage::Proxy, Class::Config));
    }
}
