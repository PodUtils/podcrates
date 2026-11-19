//! Whether a relay host offers the resumable layer on its forward road: the
//! field `forward.layer` of its `/relays.json`, `true` (podssh T-173). A relay of the
//! contract has no such field, and a document that cannot be read offers
//! nothing, so a client asks for the layer only where a host said so.
//!
//! Beside it, `forward.host` names the process that answered, where several
//! share one name: a resume must reach the process that holds its session,
//! so each link of a session goes to that name. A client takes it only when
//! it is the host that it asked, with another port, or a host of its
//! deployment, by the pool's rule (`crate::pool::same_deployment`): a token
//! goes where the user's relay sends it, and no further.

use std::time::Duration;

use pod_proxy::http;
use pod_ws::Trust;

use crate::context::Context;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::relay::{Relay, parse_relay};
use pod_session::resume::MOVE_BYTES;

/// The longest name of a host that a refusal of it shows.
const SHOWN_MOST: usize = 80;

/// What a host's `/relays.json` offers of the layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    /// Whether it offers the layer.
    pub layer: bool,
    /// The process that holds each session of it, when the host named one
    /// that a client takes.
    pub at: Option<Relay>,
    /// Why a name that the host gave is not taken, for a line: not a host,
    /// or not of its deployment.
    pub refused: Option<String>,
    /// The UDP port where the host serves podssh's QUIC (`forward.quic`,
    /// podssh T-667), which the race of QUIC takes for a host that the user named.
    pub quic: Option<u16>,
    /// The pace of Ping that a host that sleeps between frames offers
    /// (`forward.liveness`, by the rule of [`crate::liveness`]; podssh
    /// T-712); `None`: the client keeps its own.
    pub liveness: Option<std::time::Duration>,
    /// The cap of the bytes of one session, both ways, of the same document
    /// (`limits.max_session_bytes`, by the rule of
    /// [`crate::pool::parse_limits`]); `None`: the host gave none.
    pub max_session_bytes: Option<u64>,
}

impl Offer {
    /// The host that each link of a session goes to: the one that the offer
    /// names, else `asked`, whose document made the offer.
    pub fn holder(&self, asked: &Relay) -> Relay {
        self.at.clone().unwrap_or_else(|| asked.clone())
    }

    /// The bytes of one link, both ways, at which the layer moves a session
    /// to a new link (`pod_session::Settings::move_bytes`): pod-session's
    /// [`MOVE_BYTES`], or three quarters of a smaller cap of the host, as
    /// [`MOVE_BYTES`] is of the default relay's cap.
    pub fn move_bytes(&self) -> u64 {
        let under = self.max_session_bytes.map(crate::relay::three_quarters);
        under.map_or(MOVE_BYTES, |n| n.min(MOVE_BYTES))
    }

    /// The most bytes that one link carries with no acknowledgement in each
    /// direction (`pod_session::Settings::window`): an eighth of the cap of
    /// the host ([`window_under`]), `None` when it gave none. A new link
    /// first sends again the bytes that the old one did not see
    /// acknowledged; with no bound, the replay buffer (4 MiB) holds more
    /// than a small cap, and the relay ends the new link with `1009 session
    /// byte cap` (PodUtils/podssh#320).
    pub fn window(&self) -> Option<usize> {
        self.max_session_bytes.map(window_under)
    }

    /// `settings` under the cap of the host: the move threshold of
    /// [`Offer::move_bytes`], and the smaller of their window and
    /// [`Offer::window`]. The old link of a move holds (`pod_session::pump::Hold`),
    /// so it carries the threshold, the far end's window and the records in
    /// flight at most: seven eighths of the cap and a record
    /// (PodUtils/podssh#325).
    pub fn fit(&self, settings: pod_session::Settings) -> pod_session::Settings {
        let window = match (settings.window, self.window()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        pod_session::Settings { move_bytes: self.move_bytes(), window, ..settings }
    }
}

/// The window of each end of the layer on a link of a relay whose cap of a
/// session is `cap` bytes, both ways: an eighth of it. The client takes it
/// from the offer ([`Offer::window`]); the far end that the relay's role
/// runs takes it from the cap of the relay, as both ends send again what
/// the other did not acknowledge. After a move is due, the old link holds,
/// and the far end sends its window at most before it waits: with the move
/// at three quarters, an eighth leaves an eighth of the cap for the records
/// in flight and the wake of the move (PodUtils/podssh#325).
pub fn window_under(cap: u64) -> usize {
    usize::try_from(cap / 8).unwrap_or(usize::MAX).max(1)
}

/// Whether the `/relays.json` body `body` offers the layer.
pub fn offered(body: &[u8]) -> bool {
    of(body, &Relay { host: String::new(), port: 443 }).layer
}

/// The offer of the `/relays.json` body `body` of `asked`.
pub fn of(body: &[u8], asked: &Relay) -> Offer {
    let doc: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let forward = doc.get("forward");
    let layer = forward.and_then(|forward| forward.get("layer")) == Some(&serde_json::Value::Bool(true));
    // A port of UDP, 1 to 65535; anything else offers no QUIC.
    let quic = forward.and_then(|forward| forward.get("quic")).and_then(serde_json::Value::as_u64);
    let quic = quic.and_then(|port| u16::try_from(port).ok()).filter(|port| *port != 0);
    let liveness = crate::liveness::pace(forward.and_then(|forward| forward.get(crate::liveness::MEMBER)));
    let max_session_bytes = crate::pool::parse_limits(body).max_session_bytes;
    let mut offer = Offer { layer, quic, liveness, max_session_bytes, ..Offer::default() };
    match forward.and_then(|forward| forward.get("host")) {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(name)) => match named(asked, name) {
            Ok(relay) => offer.at = Some(relay),
            Err(why) => offer.refused = Some(why),
        },
        Some(_) => offer.refused = Some("forward.host is not a host's name".into()),
    }
    offer
}

/// The host that `text` names for the sessions of `asked`, `HOST[:PORT]`:
/// `asked`'s host with any port, or another host of its deployment; an
/// address names its own host alone, as an address has no deployment.
pub fn named(asked: &Relay, text: &str) -> Result<Relay, String> {
    let shown: String = pod_core::text::one_line(text).chars().take(SHOWN_MOST).collect();
    let relay = parse_relay(text).map_err(|_| format!("{shown:?} is not a host's name"))?;
    let literal = asked.host.parse::<std::net::IpAddr>().is_ok();
    let same = relay.host.eq_ignore_ascii_case(&asked.host);
    if same || (!literal && crate::pool::same_deployment(&asked.host, &relay.host)) {
        return Ok(relay);
    }
    Err(format!("{shown:?} is not of {}'s deployment", crate::relay::url_host(&asked.host)))
}

/// Ask `relay`, over TLS through the way of `ctx`, within `timeout` for each
/// step: what it offers of the layer. `Err` says why its document was not
/// read.
pub async fn ask(ctx: &Context, relay: &Relay, trust: &Trust, timeout: Duration) -> Result<Offer, String> {
    if ctx.settings.offline {
        let names = &ctx.settings.names;
        return Err(format!("{} is set, so {} does not connect anywhere", names.offline, names.app));
    }
    let extra = ctx.way.extra(&relay.host).map_err(|e| e.to_string())?;
    let (mut tls, _) =
        ctx.way.open_tls(&relay.host, relay.port, &relay.host, trust, timeout, &[]).await.map_err(|e| e.to_string())?;
    let host_header = crate::pool::host_header(relay);
    tokio::time::timeout(timeout, over_with(&mut tls, &ctx.way.user_agent, &host_header, relay, &extra))
        .await
        .map_err(|_| format!("/relays.json took more than {} s", timeout.as_secs()))?
}

/// Ask over `stream`, a connection to `asked`: its `/relays.json`, read up
/// to the pool's bound on the body. A status that is not 2xx is a relay with
/// no such document, which offers nothing.
pub async fn over<S>(stream: &mut S, user_agent: &str, host_header: &str, asked: &Relay) -> Result<Offer, String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    over_with(stream, user_agent, host_header, asked, &pod_ws::Extra::default()).await
}

/// [`over`], with the headers of the caller for the host (`extra`): a gate
/// in front of the relay refuses the document without them.
pub async fn over_with<S>(
    stream: &mut S,
    user_agent: &str,
    host_header: &str,
    asked: &Relay,
    extra: &pod_ws::Extra,
) -> Result<Offer, String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut accept = vec![("Accept", "application/json")];
    accept.extend(extra.iter());
    let max = crate::pool::MAX_BODY;
    let response = http::exchange(stream, user_agent, "GET", host_header, "/relays.json", &accept, b"", max)
        .await
        .map_err(|e| format!("/relays.json: {e}"))?;
    if !(200..300).contains(&response.status) {
        return Ok(Offer::default());
    }
    Ok(of(&response.body, asked))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window of a host with a cap is an eighth of it, and `fit` keeps
    /// a smaller window of the settings; a host with no cap leaves the
    /// window as it was (PodUtils/podssh#320). The move and the window
    /// together are seven eighths of the cap (PodUtils/podssh#325).
    #[test]
    fn the_window_of_a_cap_is_an_eighth_of_it() {
        let capped = Offer { max_session_bytes: Some(1 << 20), ..Offer::default() };
        assert_eq!(capped.window(), Some(128 << 10));
        let fit = capped.fit(pod_session::Settings::default());
        assert_eq!((fit.move_bytes, fit.window), (capped.move_bytes(), Some(128 << 10)));
        assert_eq!(fit.move_bytes + (128 << 10), 7 * (1 << 20) / 8);
        let small = pod_session::Settings { window: Some(64 << 10), ..pod_session::Settings::default() };
        assert_eq!(capped.fit(small).window, Some(64 << 10));
        assert_eq!(Offer::default().fit(small).window, Some(64 << 10));
        assert_eq!(Offer::default().fit(pod_session::Settings::default()).window, None);
    }

    fn relay(host: &str, port: u16) -> Relay {
        Relay { host: host.into(), port }
    }

    /// `forward.liveness` by the rule of `crate::liveness` (T-845).
    #[test]
    fn the_offer_keeps_the_pace_of_liveness_of_the_host() {
        let asked = relay("relay.test", 443);
        let body = br#"{"forward":{"layer":true,"liveness":{"frame":"ws-ping","pingMs":45000}}}"#;
        assert_eq!(of(body, &asked).liveness, Some(std::time::Duration::from_secs(45)));
        let fast = br#"{"forward":{"layer":true,"liveness":{"frame":"ws-ping","pingMs":5000}}}"#;
        assert_eq!(of(fast, &asked).liveness, None, "under 30 s is not taken");
        assert_eq!(of(br#"{"forward":{"layer":true}}"#, &asked).liveness, None);
    }

    /// `limits.max_session_bytes` of the same document: the layer moves at
    /// three quarters of a small cap, and at pod-session's own bytes under
    /// a large cap or none (PodUtils/podssh#314).
    #[test]
    fn the_offer_moves_the_session_under_the_cap_of_its_document() {
        let asked = relay("relay.test", 443);
        let with_cap = |cap: &str| format!(r#"{{"forward":{{"layer":true}},"limits":{{"max_session_bytes":{cap}}}}}"#);
        for (cap, moves_at) in [(1u64 << 20, 768u64 << 10), (8 << 20, 6 << 20), (1 << 40, MOVE_BYTES)] {
            let offer = of(with_cap(&cap.to_string()).as_bytes(), &asked);
            assert_eq!(offer.max_session_bytes, Some(cap));
            assert_eq!(offer.move_bytes(), moves_at, "under a cap of {cap}");
        }
        for odd in ["0", "\"1048576\"", "-1"] {
            let offer = of(with_cap(odd).as_bytes(), &asked);
            assert_eq!((offer.max_session_bytes, offer.move_bytes()), (None, MOVE_BYTES), "{odd}");
        }
        assert_eq!(of(br#"{"forward":{"layer":true}}"#, &asked).move_bytes(), MOVE_BYTES);
        assert_eq!(Offer::default().move_bytes(), pod_session::Settings::default().move_bytes);
    }

    #[test]
    fn only_a_true_of_forward_layer_is_an_offer() {
        assert!(offered(br#"{"service":"tcp-ssh-relay","forward":{"layer":true}}"#));
        for body in [
            &br#"{"service":"tcp-ssh-relay","pool":[]}"#[..],
            br#"{"forward":{"layer":"yes"}}"#,
            br#"{"forward":{"layer":false}}"#,
            br#"{"forward":true}"#,
            br#"{"layer":true}"#,
            b"not json",
            b"",
        ] {
            assert!(!offered(body), "{}", String::from_utf8_lossy(body));
        }
    }

    /// Each link of a session goes to the process that named itself, where
    /// several share the name that the user gave (podssh T-173).
    #[test]
    fn a_host_of_the_deployment_that_the_offer_names_holds_each_session() {
        let asked = relay("relay.example.org", 443);
        let body = br#"{"forward":{"layer":true,"host":"relay-1.relay.example.org"}}"#;
        let offer = of(body, &asked);
        assert_eq!((offer.layer, offer.at.clone()), (true, Some(relay("relay-1.relay.example.org", 443))));
        assert_eq!(offer.holder(&asked), relay("relay-1.relay.example.org", 443));
        let offer = of(br#"{"forward":{"layer":true,"host":"Relay.Example.org:8443"}}"#, &asked);
        assert_eq!(offer.at, Some(relay("relay.example.org", 8443)), "the same host, another port");
        let loopback = relay("127.0.0.1", 9000);
        let offer = of(br#"{"forward":{"layer":true,"host":"127.0.0.1:9001"}}"#, &loopback);
        assert_eq!(offer.holder(&loopback), relay("127.0.0.1", 9001), "an address names its own host");
        let none = of(br#"{"forward":{"layer":true,"host":null}}"#, &asked);
        assert_eq!(none.holder(&asked), asked);
        assert_eq!((none.at, none.refused), (None, None));
    }

    /// A name of another deployment, or no host at all, is not taken: the
    /// session goes to the host that the user named, and a line says why.
    #[test]
    fn a_name_of_another_deployment_or_no_host_is_not_taken() {
        let asked = relay("relay.example.org", 443);
        for (host, says) in [
            ("relay.example.com", "not of relay.example.org's deployment"),
            ("evil.org", "not of relay.example.org's deployment"),
            ("a/b", "is not a host's name"),
            ("relay-1.relay.example.org:0", "is not a host's name"),
        ] {
            let body = format!(r#"{{"forward":{{"layer":true,"host":"{host}"}}}}"#);
            let offer = of(body.as_bytes(), &asked);
            assert_eq!((offer.layer, offer.at.clone()), (true, None), "{host}");
            assert!(offer.refused.as_deref().is_some_and(|why| why.contains(says)), "{host}: {:?}", offer.refused);
            assert_eq!(offer.holder(&asked), asked, "{host}");
        }
        let literal = relay("127.0.0.1", 9000);
        let offer = of(br#"{"forward":{"layer":true,"host":"10.0.0.1:9000"}}"#, &literal);
        assert_eq!(offer.at, None, "an address has no deployment: {:?}", offer.refused);
        let offer = of(br#"{"forward":{"layer":true,"host":7}}"#, &asked);
        assert_eq!((offer.at, offer.refused.as_deref()), (None, Some("forward.host is not a host's name")));
        let long = format!(r#"{{"forward":{{"layer":true,"host":"x\u001b[2J{}.example.com"}}}}"#, "a".repeat(300));
        let refused = of(long.as_bytes(), &asked).refused.unwrap_or_default();
        assert!(refused.len() < 200 && !refused.contains('\u{1b}'), "a refusal shows a short line: {refused:?}");
    }

    /// `forward.quic` offers QUIC at a port of UDP, with or without the
    /// layer; a value that is no port offers none (podssh T-667).
    #[test]
    fn forward_quic_names_the_port_of_the_role_s_quic() {
        let asked = relay("relay.example.org", 443);
        let offer = of(br#"{"forward":{"layer":false,"host":null,"quic":443}}"#, &asked);
        assert_eq!((offer.layer, offer.quic), (false, Some(443)));
        assert_eq!(of(br#"{"forward":{"layer":true,"quic":8443}}"#, &asked).quic, Some(8443));
        for body in [
            &br#"{"forward":{"layer":true}}"#[..],
            br#"{"forward":{"quic":0}}"#,
            br#"{"forward":{"quic":70000}}"#,
            br#"{"forward":{"quic":"443"}}"#,
            br#"{"forward":{"quic":-1}}"#,
            br#"{"quic":443}"#,
        ] {
            assert_eq!(of(body, &asked).quic, None, "{}", String::from_utf8_lossy(body));
        }
    }
}
