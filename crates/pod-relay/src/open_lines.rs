//! The words of a failed host attempt: [`OpenError::lines`].

use pod_proxy::DialError;
use pod_ws::ConnectError;

use super::{HOST_DEADLINE, OpenError, is_policy_refusal};
use crate::token::Origin;

impl OpenError {
    /// What to print, one message per line, with no prefix of a program.
    /// `target` is the `host:port` the session was for.
    pub fn lines(&self, target: &str) -> Vec<String> {
        match self {
            OpenError::Offline => vec!["the offline setting is on, so no connection goes anywhere".to_string()],
            OpenError::Deadline => {
                vec![format!("the relay host did not answer within {} s", HOST_DEADLINE.as_secs())]
            }
            OpenError::Held { left } => vec![format!("{}; trying the next host", crate::gate::words(*left))],
            OpenError::Token(e) => vec![e.to_string()],
            OpenError::Connect { error, origin } => {
                // A refusal that names its cause and its target is one line:
                // the relay's words and a hint that says them again are one
                // cause said twice (PodUtils/podssh#309).
                match error {
                    ConnectError::Refused { status: status @ (400 | 403), body } if is_policy_refusal(body) => {
                        return vec![format!("the relay's policy does not allow {target} (HTTP {status})")];
                    }
                    ConnectError::Refused { status: 502, body } => {
                        return vec![match unreachable_cause(body, target) {
                            Some(cause) => format!("the relay could not reach {target}: {cause}"),
                            None => format!("the relay could not reach {target} (HTTP 502)"),
                        }];
                    }
                    _ => {}
                }
                let mut lines = vec![error.to_string()];
                let hint = match error {
                    ConnectError::Refused { status: 403, .. } if *origin == Origin::Environment => {
                        Some("the relay token that was given was rejected".to_string())
                    }
                    ConnectError::Dial(DialError::ProxyRefused { status, .. }) if *status >= 500 => Some(format!(
                        "the proxy could not reach the relay (it answered {status}); another relay host may work"
                    )),
                    ConnectError::Dial(DialError::ProxyRefused { .. }) => {
                        Some("the proxy does not allow connections to the relay".to_string())
                    }
                    _ => None,
                };
                lines.extend(hint);
                lines
            }
        }
    }
}

/// The cause in the words of a relay's `502` for `target`, with no part that
/// names the relay or the target again: `relay: cannot reach T: T: Connection
/// refused` gives `Connection refused`. `None` when nothing is left.
fn unreachable_cause(body: &str, target: &str) -> Option<String> {
    let line = pod_core::text::one_line(body);
    let mut rest = line.as_str();
    let echoes = ["relay:", "cannot reach", target, ":"];
    while let Some(after) = echoes.iter().filter(|p| !p.is_empty()).find_map(|p| rest.trim_start().strip_prefix(p)) {
        rest = after;
    }
    let rest = rest.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}
