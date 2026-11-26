//! Why a pairing call failed, and the words of each refusal.

use pod_ws::ConnectError;

use crate::relay::check_node_name;

/// The most characters of a refusal's body in a detail: the relay's words
/// on one line, never a whole page.
const DETAIL_MAX: usize = 200;

/// Why a pairing call failed. No variant holds a token, and none quotes the
/// body of a 2xx answer, which holds them. Each `detail` is empty, or `: `
/// and the relay's own words on one line.
#[derive(Debug)]
pub enum PairError {
    Connect(ConnectError),
    /// `429`: the brake that pairs share with mints, or a key past its share
    /// of podssh's relay role (podssh T-630), with what it said.
    RateLimited {
        retry_after: Option<String>,
        detail: String,
    },
    /// `503`: the relay makes no pairs, with what it said (an answer that is
    /// not a 2xx holds no token). Seen once, and gone at the
    /// next request.
    NotIssued {
        detail: String,
    },
    /// `403` for a pair's token, with what the relay said: a wrong token, an
    /// expired pair, a stopped one and a spent token of one upgrade look the
    /// same. `once`: the token is the connect token of a pair of one upgrade
    /// (`once::create_with`), so a spent token is named too.
    Forbidden {
        detail: String,
        once: bool,
    },
    /// `403` from `/v1/pair`, with what the relay said: the relay made no
    /// pair, so none is stopped or expired. A relay that makes pairs for the
    /// keys of a list alone refuses each other key.
    NotMade {
        detail: String,
    },
    /// The pair has less than 10 minutes left.
    ShortLived {
        left_ms: i64,
    },
    /// A 2xx answer that is not a usable pair, with what is wrong with it.
    BadAnswer(&'static str),
    /// A label that cannot be a file name, no pair under a label, or a
    /// name or a token of [`super::Pair::for_node`] that does not have its shape.
    BadLabel(String),
    /// The pair could not be kept or read in a private file.
    Store(String),
    Failed {
        status: u16,
        detail: String,
    },
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairError::Connect(e) => write!(f, "{e}"),
            PairError::RateLimited { retry_after: Some(s), detail } => {
                write!(f, "the relay is limiting new pairs; try again in {s} s{detail}")
            }
            PairError::RateLimited { retry_after: None, detail } => {
                write!(f, "the relay is limiting new pairs; try again later{detail}")
            }
            PairError::NotIssued { detail } => write!(f, "the relay makes no pairs (503){detail}"),
            PairError::Forbidden { detail, once: false } => {
                write!(f, "the relay refused the token (403{detail}): the pair is stopped or has expired")
            }
            PairError::Forbidden { detail, once: true } => write!(
                f,
                "the relay refused the token (403{detail}): the pair's connect token is for one upgrade, and that \
                 upgrade is used; or the pair is stopped or has expired"
            ),
            PairError::NotMade { detail } => write!(
                f,
                "the relay refused to make a pair (403{detail}): a relay that makes pairs for the keys of a list \
                 alone refuses each other key"
            ),
            PairError::ShortLived { left_ms } => {
                write!(f, "the pair has {} min left, under the 10 min that a node needs", left_ms / 60_000)
            }
            PairError::BadAnswer(what) => write!(f, "the relay's answer is not a usable pair: {what}"),
            PairError::BadLabel(why) | PairError::Store(why) => write!(f, "{why}"),
            PairError::Failed { status, detail } => write!(f, "the relay answered {status}{detail}"),
        }
    }
}

impl std::error::Error for PairError {}

impl PairError {
    /// This error, with a `403` named as one for the connect token of a pair
    /// of one upgrade when `once` is set.
    pub(crate) fn for_once(self, once: bool) -> PairError {
        match self {
            PairError::Forbidden { detail, .. } => PairError::Forbidden { detail, once },
            other => other,
        }
    }
}

/// An answer that is not a 2xx, for a pair's token. Its body is quoted (it
/// holds no token), safe to print, on one line and short.
pub(crate) fn refused(status: u16, response: &pod_proxy::http::Response) -> PairError {
    let body = response.body_text(DETAIL_MAX);
    let detail = if body.is_empty() { String::new() } else { format!(": {body}") };
    match status {
        403 => PairError::Forbidden { detail, once: false },
        // The brake's words, or a key's share of podssh's relay role (podssh T-630).
        429 => PairError::RateLimited { retry_after: response.header_text("retry-after", 64), detail },
        503 => PairError::NotIssued { detail },
        _ => PairError::Failed { status, detail },
    }
}

/// An answer of `/v1/pair` that is not a 2xx: a `403` there names no pair.
pub(crate) fn not_made(status: u16, response: &pod_proxy::http::Response) -> PairError {
    match refused(status, response) {
        PairError::Forbidden { detail, .. } => PairError::NotMade { detail },
        other => other,
    }
}

/// A label that the store takes: a name that podssh T-076 accepts, so it
/// cannot leave the cache directory. The refusal says "the label" once, in
/// the place of the words of a pair name.
pub(crate) fn check_label(label: &str) -> Result<(), PairError> {
    check_node_name(label).map_err(|why| {
        let why = why.strip_prefix("the pair name ").unwrap_or(&why);
        PairError::BadLabel(format!("the label {why}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A label that is no name is said with "the label" once (PodUtils/podssh#362).
    #[test]
    fn a_bad_label_is_said_once() {
        let long = "a".repeat(129);
        for (label, said) in [
            ("", "the label is empty"),
            (long.as_str(), "the label is longer than 128 characters"),
            ("-x", "the label \"-x\" is not a pair name"),
            ("a/b", "the label \"a/b\" is not a pair name ('/' is not allowed)"),
        ] {
            assert_eq!(check_label(label).unwrap_err().to_string(), said, "{label:?}");
        }
        assert!(check_label("lab").is_ok());
    }
}
