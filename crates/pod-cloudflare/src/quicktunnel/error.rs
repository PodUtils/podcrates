//! The failure of a provision: what a retry can do, the refusal as fields,
//! and its stage and class.

use super::*;

/// The failure of a provision. Only `BadEndpoint` comes before a network
/// call.
#[derive(Debug)]
pub enum ProvisionError {
    /// The provisioning endpoint is not a valid absolute URL.
    BadEndpoint(String),
    /// The HTTP request failed.
    Http(Failure),
    /// An HTTP status that is not 2xx.
    Status {
        code: u16,
        body: String,
        /// The `Retry-After` of the answer.
        retry_after: Option<std::time::Duration>,
    },
    /// A 2xx response that is not valid JSON, or not a `QuickTunnelResponse`.
    Decode(String),
    /// The service gave `"success": false`.
    Service(Vec<QuickTunnelError>),
    /// `success: true`, but no `result` object.
    MissingResult,
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvisionError::BadEndpoint(e) => write!(f, "invalid quick-service endpoint: {e}"),
            ProvisionError::Http(_) => write!(f, "request to the quick-tunnel service failed"),
            ProvisionError::Status { code, body, retry_after } => {
                write!(f, "quick-tunnel service returned HTTP {code}")?;
                if let Some(wait) = retry_after {
                    write!(f, " (Retry-After: {} s)", wait.as_secs())?;
                }
                if !body.is_empty() {
                    write!(f, ": {body}")?;
                }
                if *code == 405 {
                    write!(f, "; a proxy that answers 405 refuses the request form: use a CONNECT proxy")?;
                }
                Ok(())
            }
            ProvisionError::Decode(e) => {
                write!(f, "could not decode the quick-tunnel response: {e}")
            }
            ProvisionError::Service(errs) => {
                let joined: Vec<String> = errs.iter().map(|e| format!("{}: {}", e.code, e.message)).collect();
                write!(f, "quick-tunnel service reported failure: {}", joined.join("; "))
            }
            ProvisionError::MissingResult => {
                write!(f, "quick-tunnel service returned success with no result object")
            }
        }
    }
}

/// The refusal of a provision, as the fields that a supervisor needs, not
/// as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The HTTP status.
    pub status: u16,
    /// The Cloudflare error code of the body (`error code: 1015`).
    pub code: Option<u32>,
    /// The `Retry-After` of the answer, in seconds.
    pub retry_after_s: Option<u64>,
    /// True for 429 and 5xx: a later try can work.
    pub retryable: bool,
}

/// The [`Refusal`] in the chain of `err`, when there is one.
pub fn retry_of(err: &Failure) -> Option<Refusal> {
    match crate::fail::find::<ProvisionError>(err)? {
        ProvisionError::Status { code, body, retry_after } => Some(Refusal {
            status: *code,
            code: body
                .to_ascii_lowercase()
                .split("error code:")
                .nth(1)
                .and_then(|r| r.trim().split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse().ok()),
            retry_after_s: retry_after.map(|d| d.as_secs()),
            retryable: *code == 429 || (500..600).contains(code),
        }),
        _ => None,
    }
}

/// The step of a provision: the text of its failure, before the words of the
/// [`ProvisionError`] in the chain.
pub const PROVISIONING: &str = "provisioning quick tunnel";

impl From<ProvisionError> for Failure {
    /// The stage of a provision is the registration with the edge. A 429 is
    /// temporary, a 5xx is the service, and another 4xx is a refusal. An
    /// answer that does not decode is the protocol. The text is the step
    /// alone: the error is the cause, so a reader that joins the chain says
    /// its words once.
    fn from(err: ProvisionError) -> Failure {
        let (stage, class) = match &err {
            ProvisionError::BadEndpoint(_) => (Stage::Host, Class::Config),
            ProvisionError::Http(inner) => (inner.stage, inner.class),
            ProvisionError::Status { code: 429, .. } => (Stage::EdgeRegister, Class::TempFail),
            ProvisionError::Status { code, .. } if (500..600).contains(code) => {
                (Stage::EdgeRegister, Class::Unavailable)
            }
            ProvisionError::Status { .. } => (Stage::EdgeRegister, Class::Refused),
            ProvisionError::Decode(_) | ProvisionError::Service(_) | ProvisionError::MissingResult => {
                (Stage::EdgeRegister, Class::Protocol)
            }
        };
        Failure::caused(stage, class, PROVISIONING, err)
    }
}

impl std::error::Error for ProvisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProvisionError::Http(e) => Some(e),
            _ => None,
        }
    }
}
