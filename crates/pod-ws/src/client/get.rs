//! The failure of a request of pod-proxy's client ([`pod_proxy::Client`])
//! as a [`ConnectError`], as the callers of the roads read it: the dial's
//! error as it was, the handshake's as a TLS road says it, and a failure of
//! the exchange as the step of the request. The client itself, with the
//! binary's dial, TLS context and user agent, is the binary's.

use std::time::Duration;

use pod_core::{Class, Failure, Stage, Target};
use pod_proxy::DialError;

use super::ConnectError;

/// The failure of a request to `host:port` within `limit`: the dial's
/// error as it was, the handshake's as [`crate::way::connect_error`] says
/// it, and a failure of the exchange (`the HTTPS request failed: ...`, `the
/// HTTPS request timed out after Ns`) as the step of the request.
pub fn request_error(failure: Failure, limit: Duration, host: &str, port: u16) -> ConnectError {
    let cause = std::error::Error::source(&failure);
    if failure.stage != Stage::Connect || cause.is_some_and(|e| e.is::<DialError>()) {
        return crate::way::connect_error(failure, limit, &Target::new(host, port));
    }
    match cause.and_then(|e| e.downcast_ref::<std::io::Error>()) {
        Some(e) => ConnectError::Http(e.to_string()),
        None if failure.class == Class::Timeout => ConnectError::Timeout { step: "the HTTPS request", after: limit },
        None => ConnectError::Http(failure.text().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pod_proxy::Cut;

    fn get_error(failure: Failure, limit: Duration) -> ConnectError {
        request_error(failure, limit, "t", 443)
    }

    /// A failure of the exchange is the step of the request, with the
    /// words of its error; the dial's error and the handshake's stay as
    /// the roads say them.
    #[test]
    fn a_failure_of_a_get_is_podssh_s_connect_error() {
        let limit = Duration::from_secs(3);
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        let text = format!("the HTTPS request failed: {reset}");
        let failed = Failure::caused(Stage::Connect, Class::of_io(reset.kind()), text, reset);
        let failed = get_error(failed, limit);
        assert!(matches!(&failed, ConnectError::Http(why) if !why.starts_with("the HTTPS")), "{failed:?}");
        assert_eq!(failed.to_string(), "the HTTPS request failed: connection reset");
        let late = Failure::new(Stage::Connect, Class::Timeout, "the HTTPS request timed out after 3s");
        let late = get_error(late, limit);
        assert_eq!(late, ConnectError::Timeout { step: "the HTTPS request", after: limit });
        assert_eq!(late.to_string(), "the HTTPS request timed out after 3s");
        let dial = DialError::Connect {
            target: "t:443".into(),
            detail: "refused".into(),
            cut: Some(Cut::Refused),
            kind: None,
        };
        assert_eq!(get_error(Failure::from(dial.clone()), limit), ConnectError::Dial(dial));
        let slow = Failure::new(Stage::Tls, Class::Timeout, "the TLS handshake timed out after 3s");
        assert_eq!(get_error(slow, limit), ConnectError::Timeout { step: "the TLS handshake", after: limit });
    }
}
