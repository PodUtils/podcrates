//! Each error of the relay client as a `pod_core::Failure`: a stage and a
//! class that tell the caller what it can do. A failure of the connection
//! keeps the stage of `pod-ws` (the proxy, DNS, TLS, the WebSocket); the
//! answers of the relay itself (its token, its refusals, its pairs) are at
//! the stage `Relay`. The error stays the cause where it is one, so that a
//! caller that knows the crate reads its kind.

use pod_core::{Class, Failure, Stage};
use pod_ws::ConnectError;

use crate::health::HealthError;
use crate::open::{self, OpenError};
use crate::token::TokenError;

/// The stage and the class of a failed connection.
fn of_connect(e: &ConnectError) -> (Stage, Class) {
    e.stage_and_class()
}

/// The stage and the class of a failure to get a token.
pub fn token(e: &TokenError) -> (Stage, Class) {
    match e {
        TokenError::BadEnvironment => (Stage::Relay, Class::Usage),
        TokenError::Connect(c) => of_connect(c),
        TokenError::RateLimited { .. } => (Stage::Relay, Class::TempFail),
        TokenError::NotIssued => (Stage::Relay, Class::Unsupported),
        TokenError::Failed { .. } => (Stage::Relay, Class::Unavailable),
    }
}

/// The stage and the class of one host's failure.
pub fn open(e: &OpenError) -> (Stage, Class) {
    match e {
        OpenError::Offline => (Stage::Relay, Class::Policy),
        OpenError::Token(t) => token(t),
        OpenError::Connect { error, .. } => of_connect(error),
        OpenError::Deadline => (Stage::Relay, Class::Timeout),
        OpenError::Held { .. } => (Stage::Relay, Class::TempFail),
    }
}

impl From<TokenError> for Failure {
    fn from(e: TokenError) -> Failure {
        let (stage, class) = token(&e);
        Failure::new(stage, class, e.to_string())
    }
}

impl From<&OpenError> for Failure {
    fn from(e: &OpenError) -> Failure {
        let (stage, class) = open(e);
        Failure::new(stage, class, e.lines("the target").join("; "))
    }
}

/// A failed open is classed by its last attempt, which stopped the run.
impl From<&open::Failure> for Failure {
    fn from(f: &open::Failure) -> Failure {
        let (stage, class) = open(f.last());
        Failure::new(stage, class, f.lines("the target").join("; "))
    }
}

impl From<HealthError> for Failure {
    fn from(e: HealthError) -> Failure {
        let (stage, class) = match &e {
            HealthError::Connect(c) => of_connect(c),
            HealthError::Answer { .. } => (Stage::Relay, Class::Protocol),
        };
        Failure::new(stage, class, e.to_string())
    }
}

#[cfg(feature = "pair")]
mod pair {
    use pod_core::{Class, Failure, Stage};

    use crate::pair::PairError;

    /// The stage and the class of a pairing call that failed.
    pub fn class(e: &PairError) -> (Stage, Class) {
        match e {
            PairError::Connect(c) => c.stage_and_class(),
            PairError::RateLimited { .. } => (Stage::Relay, Class::TempFail),
            PairError::NotIssued { .. } => (Stage::Relay, Class::Unsupported),
            PairError::Forbidden => (Stage::Relay, Class::Refused),
            PairError::ShortLived { .. } => (Stage::Relay, Class::Revoked),
            PairError::BadAnswer(_) => (Stage::Relay, Class::Protocol),
            PairError::BadLabel(_) => (Stage::Relay, Class::Usage),
            PairError::Store(_) => (Stage::Host, Class::Config),
            PairError::Failed { .. } => (Stage::Relay, Class::Unavailable),
        }
    }

    impl From<PairError> for Failure {
        fn from(e: PairError) -> Failure {
            let (stage, class) = class(&e);
            Failure::caused(stage, class, e.to_string(), e)
        }
    }
}

#[cfg(feature = "pair")]
pub use pair::class as pair;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_answer_of_the_relay_is_at_the_stage_of_the_relay_with_a_class() {
        let cases = [
            (OpenError::Offline, Class::Policy),
            (OpenError::Deadline, Class::Timeout),
            (OpenError::Held { left: None }, Class::TempFail),
            (OpenError::Token(TokenError::BadEnvironment), Class::Usage),
            (OpenError::Token(TokenError::NotIssued), Class::Unsupported),
            (OpenError::Token(TokenError::RateLimited { retry_after: None }), Class::TempFail),
        ];
        for (e, want) in cases {
            let failure = Failure::from(&e);
            assert_eq!((failure.stage, failure.class), (Stage::Relay, want), "{e:?}");
        }
    }

    #[test]
    fn a_failed_connection_keeps_the_stage_of_pod_ws() {
        let refused = ConnectError::Refused { status: 403, body: "forward: missing or wrong token".into() };
        let want = refused.stage_and_class();
        let e = OpenError::Connect { error: refused, origin: crate::token::Origin::Cache };
        let failure = Failure::from(&e);
        assert_eq!((failure.stage, failure.class), want);
        let failed = open::Failure { attempts: vec![("a".into(), OpenError::Deadline), ("b".into(), e)] };
        let failure = Failure::from(&failed);
        assert_eq!((failure.stage, failure.class), want, "the last attempt classes the run");
    }
}
