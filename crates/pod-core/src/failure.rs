//! A failure with its stage and its class. The stage names the part of the
//! path that broke (podnet calls it the hop); the class says what a caller
//! can do (podnet maps it to an exit code). A consumer reads both from the
//! error that a crate returns, and keeps its own words and its own codes.

use std::error::Error;
use std::fmt;
use std::io;

/// The part of the path that broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The HTTP or SOCKS5 proxy.
    Proxy,
    /// Name resolution.
    Dns,
    /// The TCP or UDP connection to the far end.
    Connect,
    /// The network ended a connection before or during the TLS handshake:
    /// a policy of the host or of its network, not trust or the clock.
    Network,
    /// TLS with the far end.
    Tls,
    /// The WebSocket upgrade or its frames.
    WebSocket,
    /// QUIC.
    Quic,
    /// The relay: its token, its host, its contract.
    Relay,
    /// The registration of a tunnel with the edge.
    EdgeRegister,
    /// An edge connection after it registered.
    EdgeStream,
    /// The origin behind a tunnel.
    Origin,
    /// WARP: its registration or its session.
    Warp,
    /// The resumable layer or the end-to-end channel.
    Session,
    /// A rendezvous: a mailbox or a transit relay.
    Rendezvous,
    /// The local host: a probe of it, its file systems, its limits.
    Host,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Proxy => "proxy",
            Stage::Dns => "dns",
            Stage::Connect => "connect",
            Stage::Network => "network",
            Stage::Tls => "tls",
            Stage::WebSocket => "websocket",
            Stage::Quic => "quic",
            Stage::Relay => "relay",
            Stage::EdgeRegister => "edge-register",
            Stage::EdgeStream => "edge-stream",
            Stage::Origin => "origin",
            Stage::Warp => "warp",
            Stage::Session => "session",
            Stage::Rendezvous => "rendezvous",
            Stage::Host => "host",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a caller can do about a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// A value that the caller gave cannot be used (a bad argument).
    Usage,
    /// A configuration that cannot be used.
    Config,
    /// A service or a path is not available.
    Unavailable,
    /// A service refused for now; a later try can work.
    TempFail,
    /// A wait ended at its time limit.
    Timeout,
    /// The far end broke the protocol.
    Protocol,
    /// The far end refused the credentials or the identity.
    Refused,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Usage => "usage",
            Class::Config => "config",
            Class::Unavailable => "unavailable",
            Class::TempFail => "tempfail",
            Class::Timeout => "timeout",
            Class::Protocol => "protocol",
            Class::Refused => "refused",
        }
    }

    /// The class of an I/O error by its kind.
    pub fn of_io(kind: io::ErrorKind) -> Class {
        use io::ErrorKind as K;
        match kind {
            K::TimedOut | K::WouldBlock => Class::Timeout,
            K::InvalidInput => Class::Usage,
            K::InvalidData | K::UnexpectedEof => Class::Protocol,
            K::PermissionDenied => Class::Refused,
            _ => Class::Unavailable,
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A failure of a crate: its stage, its class, its text and its cause.
#[derive(Debug)]
pub struct Failure {
    pub stage: Stage,
    pub class: Class,
    text: String,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl Failure {
    pub fn new(stage: Stage, class: Class, text: impl Into<String>) -> Failure {
        Failure { stage, class, text: text.into(), source: None }
    }

    /// A failure whose cause is `source`. The text comes before the text of
    /// the cause in [`Failure::chain`].
    pub fn caused(
        stage: Stage,
        class: Class,
        text: impl Into<String>,
        source: impl Into<Box<dyn Error + Send + Sync + 'static>>,
    ) -> Failure {
        Failure { stage, class, text: text.into(), source: Some(source.into()) }
    }

    /// An I/O error at `stage`, with the class of its kind.
    pub fn io(stage: Stage, e: io::Error) -> Failure {
        let class = Class::of_io(e.kind());
        Failure { stage, class, text: e.to_string(), source: Some(Box::new(e)) }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text and the text of each cause, joined with `: `. A cause whose
    /// text is in the chain already is not named again: a text that quotes
    /// its cause and adds what to do (the hint of a TLS failure) stays as it
    /// is.
    pub fn chain(&self) -> String {
        let mut out = self.text.clone();
        let mut next = self.source.as_deref().map(|e| e as &(dyn Error + 'static));
        while let Some(e) = next {
            let more = e.to_string();
            if !out.contains(&more) {
                out.push_str(": ");
                out.push_str(&more);
            }
            next = e.source();
        }
        out
    }

    /// The same failure at another stage, for a layer that knows better
    /// where it broke.
    pub fn at(mut self, stage: Stage) -> Failure {
        self.stage = stage;
        self
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|e| e as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_io_error_keeps_its_kind_as_a_class() {
        let f = Failure::io(Stage::Connect, io::Error::new(io::ErrorKind::TimedOut, "no answer"));
        assert_eq!((f.stage, f.class), (Stage::Connect, Class::Timeout));
        assert_eq!(f.to_string(), "no answer");
        let refused = Failure::io(Stage::Connect, io::Error::from(io::ErrorKind::ConnectionRefused));
        assert_eq!(refused.class, Class::Unavailable);
    }

    #[test]
    fn the_chain_names_each_cause_once() {
        let cause = io::Error::new(io::ErrorKind::ConnectionReset, "reset by peer");
        let f = Failure::caused(Stage::Tls, Class::Unavailable, "the handshake ended", cause);
        assert_eq!(f.chain(), "the handshake ended: reset by peer");
        assert!(f.source().is_some());
        let same = Failure::caused(Stage::Dns, Class::Unavailable, "no answer", io::Error::other("no answer"));
        assert_eq!(same.chain(), "no answer");
        let quoted = io::Error::other("invalid peer certificate: UnknownIssuer");
        let hinted = "invalid peer certificate: UnknownIssuer; give the CA of the proxy";
        let f = Failure::caused(Stage::Tls, Class::Protocol, hinted, quoted);
        assert_eq!(f.chain(), hinted);
    }

    #[test]
    fn a_layer_can_move_the_stage() {
        let f = Failure::new(Stage::Connect, Class::Timeout, "slow").at(Stage::Proxy);
        assert_eq!(f.stage.name(), "proxy");
        assert_eq!(f.class.to_string(), "timeout");
    }
}
