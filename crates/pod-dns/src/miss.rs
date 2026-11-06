//! Why a question got no answer, and the reading of a resolver's answer
//! into what it gives or why it gives nothing.

use pod_proxy::http::Response;
use pod_proxy::{Cut, Missed};

use crate::NXDOMAIN;

/// Why a question got no answer: the way to the resolver failed, and the
/// other form would meet the same, with how its connection ended where I/O
/// ended it; the answer was not of the form asked, and the other form may
/// work; or the resolver said why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Miss {
    Way(String, Option<Cut>),
    Form(String),
    Said(String),
}

/// A failure of the way in words alone.
pub(crate) fn way(why: String) -> Miss {
    Miss::Way(why, None)
}

impl From<Missed> for Miss {
    fn from(missed: Missed) -> Miss {
        Miss::Way(missed.why, missed.cut)
    }
}

/// What an answer gives, by `parse`, or why it gives nothing. The status of a
/// resolver that is busy or broken is the way's: the other form would meet it.
pub(crate) fn read<T>(response: &Response, parse: impl FnOnce(&[u8]) -> Result<T, String>) -> Result<T, Miss> {
    match response.status {
        200 => parse(&response.body).map_err(|e| if said(&e) { Miss::Said(e) } else { Miss::Form(e) }),
        status @ (429 | 500..=599) => Err(way(format!("HTTP {status}"))),
        status => Err(Miss::Form(format!("HTTP {status}"))),
    }
}

/// Whether the text of a parse is a resolver's own status, rather than an
/// answer of another form than the one asked.
pub(crate) fn said(e: &str) -> bool {
    e == NXDOMAIN || e.strip_prefix("DNS status ").is_some_and(|n| n.parse::<u16>().is_ok())
}
