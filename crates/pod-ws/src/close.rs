//! What a Close carries: its payload, written and read; the codes of RFC
//! 6455 section 7.4.1 that the relays use, and a hint for each; and what the
//! relay's Close of a forward session means.

use crate::frame;

/// The close codes of RFC 6455 section 7.4.1 that a relay sends.
pub mod code {
    /// The session ended as asked.
    pub const NORMAL: u16 = 1000;
    /// The far end goes away (a restart, an idle limit, a time cap).
    pub const GOING_AWAY: u16 = 1001;
    /// A frame or a message that RFC 6455 forbids.
    pub const PROTOCOL: u16 = 1002;
    /// A message over a limit (the relay: its byte cap or its frame cap).
    pub const TOO_BIG: u16 = 1009;
    /// An error of the far end (the relay: its connection to the target).
    pub const ERROR: u16 = 1011;
    /// The far end is busy: try again later.
    pub const TRY_LATER: u16 = 1013;
}

/// What a close code of a relay means, for the end of an error line: empty
/// for a code with no hint. The text starts with a space.
pub fn close_hint(code: u16) -> &'static str {
    match code {
        code::GOING_AWAY => " (the relay goes away; open a new session)",
        code::TOO_BIG => " (the session passed the byte limit of the relay; open a new session)",
        code::ERROR => " (an error of the relay)",
        code::TRY_LATER => " (the relay is busy; try again later)",
        _ => "",
    }
}

/// A Close payload: the code (big-endian), then a reason cut to fit the
/// 125-byte control-frame limit on a character boundary.
pub fn close_payload(code: Option<u16>, reason: &str) -> Vec<u8> {
    let Some(code) = code else { return Vec::new() };
    let mut out = code.to_be_bytes().to_vec();
    let mut end = reason.len().min(frame::MAX_CONTROL_PAYLOAD - 2);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    out.extend_from_slice(&reason.as_bytes()[..end]);
    out
}

/// The status code and reason of a received Close frame's payload.
pub fn close_code_and_reason(payload: &[u8]) -> (Option<u16>, String) {
    if payload.len() < 2 {
        return (None, String::new());
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    // The reason is the peer's text and is printed to the user's terminal.
    (Some(code), pod_core::text::one_line(&String::from_utf8_lossy(&payload[2..])))
}

/// The part of a forward session that a Close of the relay names, by its
/// code and reason (the relay's own words; `docs/relay.md`, "Errors and
/// close codes"). The SSH road and the proxy road of a client both use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardClose {
    /// 1000: the target or the client ended the session.
    Normal,
    /// A limit of the relay: 1001 (idle, session time) or 1009 (bytes, frame).
    Limit,
    /// The relay's connection to the target failed or broke.
    Target,
    /// The connection between the client and the relay broke; also no Close.
    Client,
    /// A code or a reason that the relay's table does not list.
    Unknown,
}

/// Classify a Close of the forward path; `None` is a connection that ended
/// with no Close.
pub fn forward_close(code: Option<u16>, reason: &str) -> ForwardClose {
    const TARGET: [&str; 4] =
        ["connect failed", "target closed before sending anything", "wrong target banner", "write failed"];
    match code {
        None => ForwardClose::Client,
        Some(1000) => ForwardClose::Normal,
        Some(1001 | 1009) => ForwardClose::Limit,
        Some(1011) if TARGET.iter().any(|t| reason.starts_with(t)) => ForwardClose::Target,
        Some(1011) if reason.starts_with("client send failed") || reason.starts_with("client error") => {
            ForwardClose::Client
        }
        Some(1013) if reason.starts_with("target write backlog") => ForwardClose::Target,
        Some(1013) if reason.starts_with("client receive backlog") => ForwardClose::Client,
        Some(_) => ForwardClose::Unknown,
    }
}

impl ForwardClose {
    /// What broke, in words: `target` is the `host:port` that the relay
    /// dials, and `app` the name of the client.
    pub fn what(self, target: &str, app: &str) -> String {
        match self {
            ForwardClose::Normal => format!("{target} ended the connection"),
            ForwardClose::Limit => "the relay ended the session at one of its limits".into(),
            ForwardClose::Target => format!("the relay lost its connection to {target}"),
            ForwardClose::Client => format!("the connection between {app} and the relay broke"),
            ForwardClose::Unknown => "the relay closed the session".into(),
        }
    }
}

#[cfg(test)]
mod forward_close_tests {
    use super::*;

    /// One case for each row of the relay's table, and an unknown code.
    #[test]
    fn forward_close_names_the_hop_for_each_row() {
        let cases = [
            (Some(1000), "target closed", ForwardClose::Normal),
            (Some(1001), "idle timeout", ForwardClose::Limit),
            (Some(1001), "session time cap", ForwardClose::Limit),
            (Some(1009), "session byte cap", ForwardClose::Limit),
            (Some(1009), "frame larger than 262144 bytes", ForwardClose::Limit),
            (Some(1011), "connect failed: refused", ForwardClose::Target),
            (Some(1011), "target closed before sending anything", ForwardClose::Target),
            (Some(1011), "wrong target banner SSH-1.5", ForwardClose::Target),
            (Some(1011), "write failed: Network connection lost.", ForwardClose::Target),
            (Some(1011), "client send failed: reset", ForwardClose::Client),
            (Some(1011), "client error", ForwardClose::Client),
            (Some(1013), "target write backlog", ForwardClose::Target),
            (Some(1013), "client receive backlog", ForwardClose::Client),
            (None, "", ForwardClose::Client),
            (Some(4321), "something new", ForwardClose::Unknown),
            (Some(1011), "a reason the table does not list", ForwardClose::Unknown),
        ];
        for (code, reason, want) in cases {
            assert_eq!(forward_close(code, reason), want, "{code:?} {reason}");
        }
        let target = "railway.new:22";
        assert_eq!(ForwardClose::Target.what(target, "podssh"), "the relay lost its connection to railway.new:22");
        assert_eq!(ForwardClose::Client.what(target, "podssh"), "the connection between podssh and the relay broke");
    }

    /// Each code that a relay sends with a hint has one; 1000 and an
    /// unknown code have none.
    #[test]
    fn close_hint_names_each_code_of_the_relay() {
        assert!(close_hint(code::TOO_BIG).contains("byte limit"));
        assert!(close_hint(code::GOING_AWAY).contains("goes away"));
        assert!(close_hint(code::ERROR).contains("error of the relay"));
        assert!(close_hint(code::TRY_LATER).contains("busy"));
        for none in [code::NORMAL, code::PROTOCOL, 4000] {
            assert_eq!(close_hint(none), "", "{none}");
        }
        assert!(close_hint(code::TOO_BIG).starts_with(' '));
    }
}
