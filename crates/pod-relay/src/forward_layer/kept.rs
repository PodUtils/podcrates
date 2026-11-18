//! What each relay host offered of the resumable layer, kept for a day
//! (podssh T-173), so that a run that did not ask for the layer takes it where its
//! first host offered it, with no read of `/relays.json` of its own: each
//! read of a host's offer keeps it (a run that asked, the pool's refresh of
//! the same document, the once-a-day read of a host that the user named).
//!
//! A kept offer is a hint, never a promise: a host whose layer path then
//! answers 404 is forgotten, and the session takes the contract's road. The
//! name of the process that holds the sessions is kept as the host gave it,
//! and checked again by [`offer::named`] at each read of it, as at the read
//! of the document.

use serde::{Deserialize, Serialize};

use super::offer::{self, Offer};
use crate::cache::{self, Cache};
use crate::relay::Relay;

/// How long a kept offer stands: a day, as a role's operator who turns the
/// layer on or off is heard by the next day's runs.
pub const FRESH_MS: i64 = 24 * 3600 * 1000;

#[derive(Serialize, Deserialize)]
struct KeptOffer {
    read_ms: i64,
    layer: bool,
    /// `forward.host` as the host gave it and as the client took it.
    #[serde(default)]
    at: Option<String>,
    /// `forward.quic`, the port of the host's QUIC (podssh T-667); none in a file
    /// of before it.
    #[serde(default)]
    quic: Option<u16>,
    /// `limits.max_session_bytes` of the same document, so that a run that
    /// takes a kept offer moves its session before the cap with no read
    /// (PodUtils/podssh#314); none in a file of before it.
    #[serde(default)]
    max_session_bytes: Option<u64>,
}

/// The cache file of `relay`'s offer.
pub fn file_name(relay: &Relay) -> String {
    format!("relay-offer-{}-{}.json", pod_core::files::exact_name(&relay.host), relay.port)
}

/// Keep `offered`, what `relay` offered at `now_ms`; a cache that takes no
/// file is no failure of the run, which reads the offer again next time.
pub fn keep(cache: &Cache, relay: &Relay, offered: &Offer, now_ms: i64) {
    let at = offered.at.as_ref().map(|at| pod_proxy::host::authority(&at.host, at.port));
    let (layer, quic, max_session_bytes) = (offered.layer, offered.quic, offered.max_session_bytes);
    let kept = KeptOffer { read_ms: now_ms, layer, at, quic, max_session_bytes };
    if let Ok(body) = serde_json::to_vec(&kept) {
        let _ = cache.store_file(&file_name(relay), &body);
    }
}

/// What `relay` offered within [`FRESH_MS`] of `now_ms`; `None` when nothing
/// is kept, or what is kept is older, or a clock went back before it.
pub fn kept(cache: &Cache, relay: &Relay, now_ms: i64) -> Option<Offer> {
    let text = cache.load_file(&file_name(relay))?;
    of_text(&text, relay, now_ms)
}

/// [`kept`] of a file's text.
fn of_text(text: &str, relay: &Relay, now_ms: i64) -> Option<Offer> {
    let kept: KeptOffer = serde_json::from_str(text).ok()?;
    let age = now_ms.checked_sub(kept.read_ms)?;
    if !(0..=FRESH_MS).contains(&age) {
        return None;
    }
    let (layer, quic, max_session_bytes) = (kept.layer, kept.quic, kept.max_session_bytes);
    let mut offered = Offer { layer, quic, max_session_bytes, ..Offer::default() };
    if let Some(at) = &kept.at {
        match offer::named(relay, at) {
            Ok(holder) => offered.at = Some(holder),
            Err(why) => offered.refused = Some(why),
        }
    }
    Some(offered)
}

/// [`keep`], now.
pub fn keep_now(cache: &Cache, relay: &Relay, offered: &Offer) {
    keep(cache, relay, offered, crate::pool::now_ms());
}

/// [`kept`], now.
pub fn kept_now(cache: &Cache, relay: &Relay) -> Option<Offer> {
    kept(cache, relay, crate::pool::now_ms())
}

/// Forget what `relay` offered: its layer path refused a session.
pub fn forget(cache: &Cache, relay: &Relay) {
    cache::remove_named_from(cache.dirs(), &file_name(relay));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay(host: &str, port: u16) -> Relay {
        Relay { host: host.into(), port }
    }

    fn text(read_ms: i64, layer: bool, at: Option<&str>) -> String {
        let at = at.map(String::from);
        serde_json::to_string(&KeptOffer { read_ms, layer, at, quic: None, max_session_bytes: None }).unwrap()
    }

    #[test]
    fn an_offer_stands_for_a_day_and_not_after_it_nor_before_it_was_read() {
        let asked = relay("relay.example.org", 443);
        let now = 1_793_577_600_000;
        let offered = of_text(&text(now - FRESH_MS, true, None), &asked, now).expect("a day old");
        assert!(offered.layer);
        assert_eq!(offered.holder(&asked), asked);
        assert!(of_text(&text(now - FRESH_MS - 1, true, None), &asked, now).is_none(), "older than a day");
        assert!(of_text(&text(now + 1, true, None), &asked, now).is_none(), "a clock that went back");
        assert!(!of_text(&text(now, false, None), &asked, now).unwrap().layer, "an offer of none is kept too");
        assert!(of_text("not json", &asked, now).is_none());
    }

    #[test]
    fn the_holder_is_checked_again_as_at_the_read_of_the_document() {
        let asked = relay("relay.example.org", 443);
        let now = 1_793_577_600_000;
        let same = of_text(&text(now, true, Some("b.example.org:8443")), &asked, now).unwrap();
        assert_eq!(same.at, Some(relay("b.example.org", 8443)));
        let foreign = of_text(&text(now, true, Some("elsewhere.test")), &asked, now).unwrap();
        assert_eq!(foreign.at, None);
        assert!(foreign.refused.unwrap().contains("is not of relay.example.org's deployment"));
    }

    #[test]
    fn each_host_and_port_has_a_file_of_its_own() {
        assert_eq!(file_name(&relay("Relay.Example.org", 443)), "relay-offer-relay.example.org-443.json");
        assert_ne!(file_name(&relay("a.test", 443)), file_name(&relay("a.test", 8443)));
        // A name with `_` and an IPv6 address have files of their own (T-246).
        assert_ne!(file_name(&relay("__1", 443)), file_name(&relay("::1", 443)));
        assert_ne!(crate::pool::file_name("__1"), crate::pool::file_name("::1"));
        assert_ne!(crate::cache::file_name("___1__8443"), crate::cache::file_name("[::1]:8443"));
    }

    /// The port of the host's QUIC is kept with the offer, and a file of
    /// before it reads as none (podssh T-667).
    #[test]
    fn the_port_of_quic_is_kept_with_the_offer() {
        let asked = relay("relay.example.org", 443);
        let now = 1_793_577_600_000;
        let kept = KeptOffer { read_ms: now, layer: false, at: None, quic: Some(443), max_session_bytes: None };
        let text = serde_json::to_string(&kept).unwrap();
        assert_eq!(of_text(&text, &asked, now).unwrap().quic, Some(443));
        let before = format!(r#"{{"read_ms":{now},"layer":true}}"#);
        assert_eq!(of_text(&before, &asked, now).unwrap().quic, None);
    }

    /// The cap of the host's sessions is kept with the offer: an offer from
    /// the cache moves its session under it with no read, and a file of
    /// before it reads as no cap (PodUtils/podssh#314).
    #[test]
    fn the_cap_of_a_session_is_kept_with_the_offer() {
        let asked = relay("relay.example.org", 443);
        let now = 1_793_577_600_000;
        let body = br#"{"forward":{"layer":true},"limits":{"max_session_bytes":2097152}}"#;
        let read = offer::of(body, &asked);
        assert_eq!(read.max_session_bytes, Some(2 << 20));
        let dir = std::env::temp_dir().join(format!("pod-relay-kept-cap-{}", std::process::id()));
        let cache = Cache::in_dirs(vec![dir.clone()]);
        keep(&cache, &asked, &read, now);
        let from_the_cache = kept(&cache, &asked, now).expect("the kept offer");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(from_the_cache, read, "the same offer, the cap too");
        assert_eq!(from_the_cache.move_bytes(), 3 << 19, "three quarters of 2 MiB");
        let before = format!(r#"{{"read_ms":{now},"layer":true}}"#);
        let old = of_text(&before, &asked, now).unwrap();
        assert_eq!((old.max_session_bytes, old.move_bytes()), (None, pod_session::resume::MOVE_BYTES));
    }
}
