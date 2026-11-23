//! Why a connection could not be opened: a [`DialError`], which names what
//! failed and the address or the proxy, and never a credential. Each one
//! becomes a [`pod_core::Failure`] with its stage and its class, and stays
//! its cause, so that a caller reads the variant and its cut back.

use std::time::Duration;

use pod_core::failure::{lead, strip_lead};
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
    /// Every address failed; the cut of the last one, and the kind of its
    /// error of I/O whole, for a caller that tells more kinds apart than
    /// the cuts (an unreachable network or host, a refusal of this host's
    /// rules).
    Connect { target: String, detail: String, cut: Option<Cut>, kind: Option<std::io::ErrorKind> },
    /// A step did not finish in time: the stage of the step (the proxy, the
    /// connection, the resolution).
    Timeout { step: String, after: Duration, stage: Stage },
    /// The proxy itself could not be reached.
    ProxyUnreachable { proxy: String, detail: String },
    /// The proxy answered CONNECT with a status other than 2xx; a SOCKS5
    /// proxy's refusal carries the HTTP status of the same case (403 for its
    /// rules, 407 for its authentication, 502 for the rest). The status 0: a
    /// dialer of the caller refused at the proxy and gave no status.
    ProxyRefused { proxy: String, target: String, status: u16, reason: String },
    /// The proxy did not answer as its protocol says.
    ProxyProtocol { proxy: String, detail: String },
    /// The proxy does not know the SOCKS5 command 0x83, UDP in the stream
    /// (reply 0x07). `UDP ASSOCIATE` is the other way.
    UdpRefused { proxy: String },
}

impl std::fmt::Display for DialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialError::InvalidTarget(why) => write!(f, "invalid destination: {why}"),
            DialError::BadProxy(why) => write!(f, "the proxy setting is not usable: {why}"),
            // A detail of a dialer of the caller can say the same words.
            DialError::Resolve { host, detail } => f.write_str(&lead(&format!("could not resolve {host}"), detail)),
            DialError::Connect { target, detail, .. } => {
                f.write_str(&lead(&format!("could not connect to {target}"), detail))
            }
            DialError::Timeout { step, after, .. } => write!(f, "{step} timed out after {}s", after.as_secs()),
            DialError::ProxyUnreachable { proxy, detail } => {
                // A detail of another dialer can name the proxy again.
                let detail = strip_lead(detail, &format!("could not connect to {proxy}"));
                write!(f, "could not reach {}: {detail}", the_proxy(proxy))
            }
            // The status 0: a dialer of the caller gave no status.
            DialError::ProxyRefused { proxy, target, status: 0, reason } if reason.is_empty() => {
                write!(f, "{} refused CONNECT {target}", the_proxy(proxy))
            }
            DialError::ProxyRefused { proxy, target, status: 0, reason } => {
                write!(f, "{} refused CONNECT {target}: {reason}", the_proxy(proxy))
            }
            DialError::ProxyRefused { proxy, target, status, reason } => {
                write!(f, "{} refused CONNECT {target}: {status} {reason}", the_proxy(proxy))
            }
            DialError::ProxyProtocol { proxy, detail } => {
                write!(f, "{} did not answer as its protocol says: {detail}", the_proxy(proxy))
            }
            DialError::UdpRefused { proxy } => {
                write!(f, "the proxy {proxy} does not know the SOCKS5 command UDP in the stream (reply 7)")
            }
        }
    }
}

/// "the proxy P", or "the proxy" when a dialer of the caller did not name it.
fn the_proxy(proxy: &str) -> String {
    if proxy.is_empty() { "the proxy".into() } else { format!("the proxy {proxy}") }
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
            DialError::UdpRefused { .. } => (Stage::Proxy, Class::Unsupported),
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
        let silent =
            DialError::Connect { target: "t:2".into(), detail: "d".into(), cut: Some(Cut::TimedOut), kind: None };
        assert_eq!(silent.stage_and_class(), (Stage::Connect, Class::Timeout));
        assert_eq!(DialError::InvalidTarget("x".into()).stage_and_class(), (Stage::Connect, Class::Usage));
        assert_eq!(DialError::BadProxy("x".into()).stage_and_class(), (Stage::Proxy, Class::Config));
    }

    /// A detail that says the words of the error already is not said
    /// twice (PodUtils/podnet#658).
    #[test]
    fn the_words_of_a_dial_error_are_said_once() {
        // A detail of another dialer that names the proxy again
        // (PodUtils/podnet#707, ask 6).
        let detail = "could not connect to 127.0.0.1:1: Connection refused (os error 111)".to_string();
        let unreachable = DialError::ProxyUnreachable { proxy: "127.0.0.1:1".into(), detail };
        assert_eq!(unreachable.to_string(), "could not reach the proxy 127.0.0.1:1: Connection refused (os error 111)");
        // A refusal at the proxy with no status, from a dialer of the caller.
        let refused = DialError::ProxyRefused {
            proxy: "http://p:1".into(),
            target: "x:443".into(),
            status: 0,
            reason: "refused".into(),
        };
        assert_eq!(refused.to_string(), "the proxy http://p:1 refused CONNECT x:443: refused");
        assert_eq!(refused.stage_and_class(), (Stage::Proxy, Class::Refused));
        let unnamed = DialError::ProxyUnreachable { proxy: String::new(), detail: "no answer".into() };
        assert_eq!(unnamed.to_string(), "could not reach the proxy: no answer");
        let detail = "could not resolve nowhere.invalid: system resolver: no answer".to_string();
        let resolve = DialError::Resolve { host: "nowhere.invalid".into(), detail: detail.clone() };
        assert_eq!(resolve.to_string(), detail);
        let detail = "could not connect to 127.0.0.1:1: refused".to_string();
        let connect =
            DialError::Connect { target: "127.0.0.1:1".into(), detail: detail.clone(), cut: None, kind: None };
        assert_eq!(connect.to_string(), detail);
        let connect =
            DialError::Connect { target: "127.0.0.1:1".into(), detail: "refused".into(), cut: None, kind: None };
        assert_eq!(connect.to_string(), detail);
    }
}
