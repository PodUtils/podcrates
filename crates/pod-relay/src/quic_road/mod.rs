//! QUIC on UDP to podssh's relay role (podssh T-667), raced with the TCP roads to
//! the same host. The role serves QUIC beside its TCP listener, on the same
//! port of UDP, with an ALPN of podssh's own ([`ALPN`]): each bidirectional
//! stream is one connection of HTTP/1.1 to the role's routes, so a session is
//! the contract's upgrade, its token, its checks and its frames, on a stream
//! in place of a TLS connection. QUIC's handshake is one round trip where TLS
//! over TCP is two, its connection ids outlive a rebinding of the NAT, and a
//! lost packet holds no other connection's bytes.
//!
//! Which sessions race it, by `Settings::quic`: unset, empty or `auto`, a
//! session to a relay host that the user named, whose `/relays.json` offered
//! QUIC (`forward.quic`, kept with the offer of the resumable layer for a
//! day, podssh T-173); `yes`, each relay host, at the port that it offered,
//! else at its own; `no`, none. In each case only where no proxy is set for
//! the host, as no HTTP proxy carries UDP, and never with a caller's own TLS
//! configuration, which the crate cannot give QUIC's ALPN.
//!
//! The race ([`race`]): QUIC first, after the quick tunnel's probe of UDP
//! (podssh T-400), which a server of QUIC answers in one round trip; the TCP road
//! after a head start of [`HEAD_START`], or at once when QUIC failed. The
//! first session kept, the loser dropped at once; an answer of the relay
//! itself (a refusal of the token or the target) on either road ends the
//! race, as the other road would get the same. A host whose UDP failed is
//! not tried over QUIC again by the run of the `Context` for [`REMEMBERED`],
//! and one whose QUIC answered is not probed again for as long. The
//! resumable layer stays above each road, so a session that QUIC loses
//! resumes on any road.

pub mod race;

#[cfg(feature = "quic")]
pub mod dial;
#[cfg(feature = "quic")]
pub mod stream;

use std::time::Duration;

use pod_ws::{ConnectError, RelaySession, WsClientConfig};

use crate::context::Context;

/// The ALPN of the relay role's QUIC: a stream is a connection of HTTP/1.1.
pub const ALPN: &[u8] = b"podssh-relay/1";
/// How long QUIC goes alone before the TCP road starts, as osewa's race of
/// QUIC and its WebSocket gives it.
pub const HEAD_START: Duration = Duration::from_millis(300);
/// How long a run keeps what it learned of UDP to a host.
pub const REMEMBERED: Duration = Duration::from_secs(600);
/// The words of a session on the road, after the relay host's name.
pub const WORDS: &str = "over QUIC";

/// The words of `Settings::quic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A relay host that the user named, where it offered QUIC.
    Auto,
    /// Each relay host.
    Yes,
    /// None.
    No,
}

impl Choice {
    /// The choice of `value`; another word is refused, with why, which
    /// names the setting `name`.
    pub fn parse(value: Option<&str>, name: &str) -> Result<Choice, String> {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("" | "auto") => Ok(Choice::Auto),
            Some("yes" | "on" | "true" | "1") => Ok(Choice::Yes),
            Some("no" | "off" | "false" | "0") => Ok(Choice::No),
            Some(_) => Err(format!("{name}: auto, yes or no")),
        }
    }

    /// [`Choice::parse`] of the settings of `ctx`.
    pub fn of(ctx: &Context) -> Result<Choice, String> {
        Choice::parse(ctx.settings.quic.as_deref(), &ctx.settings.names.quic)
    }

    /// The UDP port of QUIC for a session to a host of a list that the user
    /// named (`explicit`), which `offered` its port, or of the default relay;
    /// `own` is the host's port of TCP. `None`: no QUIC.
    pub fn port(self, explicit: bool, offered: Option<u16>, own: u16) -> Option<u16> {
        match self {
            Choice::Auto if explicit => offered,
            Choice::Auto | Choice::No => None,
            Choice::Yes => Some(offered.unwrap_or(own)),
        }
    }
}

/// Whether `e` is the relay's own answer, or a fault of the request, which
/// each road of the race would get alike: it ends the race.
pub fn decisive(e: &ConnectError) -> bool {
    matches!(e, ConnectError::Refused { .. } | ConnectError::Config(_))
}

/// The session of `config` with `token` to a host of a list that the user
/// named when `explicit`: QUIC raced with the TCP road where [`Choice`] and
/// the host offer it, else the TCP road alone, as
/// [`crate::h2_road::connect`] makes it.
pub async fn connect(
    ctx: &Context,
    config: &WsClientConfig,
    token: &str,
    explicit: bool,
) -> Result<RelaySession, ConnectError> {
    #[cfg(feature = "quic")]
    if let Some(port) = dial::offered(ctx, config, explicit) {
        // On the heap: each road's future is large, and both on one task's
        // stack overflowed a worker's in a debug build.
        let quic = Box::pin(dial::session(ctx, config, token, port));
        let tcp = Box::pin(crate::h2_road::connect(ctx, config, token, explicit));
        return race::race(quic, tcp, HEAD_START, decisive).await.result;
    }
    crate::h2_road::connect(ctx, config, token, explicit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unset is auto: a host that the user named and that offered QUIC; yes
    /// takes each host, at its own port where it offered none; another word
    /// is refused.
    #[test]
    fn the_variable_and_the_offer_say_which_sessions_race_quic() {
        let name = "PODSSH_QUIC";
        for (value, choice) in [(None, Choice::Auto), (Some(" "), Choice::Auto), (Some("AUTO"), Choice::Auto)] {
            assert_eq!(Choice::parse(value, name), Ok(choice), "{value:?}");
        }
        assert_eq!(Choice::parse(Some("yes"), name), Ok(Choice::Yes));
        assert_eq!(Choice::parse(Some("off"), name), Ok(Choice::No));
        assert_eq!(Choice::parse(Some("udp"), name), Err("PODSSH_QUIC: auto, yes or no".into()));
        assert_eq!(Choice::Auto.port(true, Some(8443), 443), Some(8443), "named, and offered");
        assert_eq!(Choice::Auto.port(true, None, 443), None, "named, and offered nothing");
        assert_eq!(Choice::Auto.port(false, Some(443), 443), None, "the default relay");
        assert_eq!(Choice::Yes.port(false, None, 443), Some(443), "yes: at the host's own port");
        assert_eq!(Choice::Yes.port(true, Some(4433), 443), Some(4433));
        assert_eq!(Choice::No.port(true, Some(443), 443), None);
    }

    /// The relay's refusal and a fault of the request end the race; a road's
    /// own failure does not.
    #[test]
    fn only_the_relay_s_answer_ends_the_race() {
        assert!(decisive(&ConnectError::Refused { status: 403, body: "forward: missing or wrong token".into() }));
        assert!(decisive(&ConnectError::Refused { status: 502, body: "relay: cannot reach x:22".into() }));
        assert!(decisive(&ConnectError::Config("the relay token contains a line break".into())));
        assert!(!decisive(&ConnectError::Upgrade("the relay closed the connection".into())));
        let late = ConnectError::Timeout { step: "the WebSocket upgrade", after: Duration::from_secs(20) };
        assert!(!decisive(&late));
    }
}
