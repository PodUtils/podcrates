//! The open of a MASQUE session: the race of the direct path and the relay
//! ([`crate::leg`]), on each leg each door in turn, and the device record
//! read again when the front door shows another key than the pinned one.

use std::time::Duration;

use pod_core::{Class, Event, Failure, Stage, Target};

use crate::device::Device;
use crate::door::Door;
use crate::door::{self, Routes};
use crate::identity::spki_from_pem;
use crate::leg::Leg;
use crate::register;
use crate::session::{MasqueSession, Way};
use crate::tls::is_pin_mismatch;
use crate::warp::Warp;

/// The pause before the last try with the key of a fresh record: a new
/// connection may reach another server.
const PIN_RETRY: Duration = Duration::from_secs(1);

/// The pinned key of a device record, as DER.
pub(crate) fn pin_of(device: &Device) -> Result<Vec<u8>, Failure> {
    let pem = device.peer_public_key_pem().ok_or_else(|| {
        Failure::new(Stage::Warp, Class::Protocol, "the device has no peer key to pin; no session opens without one")
    })?;
    spki_from_pem(pem)
}

impl Warp {
    /// Open a session. With [`crate::Settings::wireguard`], the WireGuard
    /// tunnel on the UDP of the way. Else a MASQUE session; with no
    /// [`crate::Settings::relay`], or over HTTP/3 ([`Way::h3`]: the relay
    /// carries TCP), on the direct path alone. With a relay, the direct path starts first and
    /// the relay starts after its delay ([`crate::leg::Relay::delay_for`]),
    /// or at once when the direct path failed; the first session that
    /// opens wins and the other try is dropped (PodUtils/podnet issues 76
    /// and 126). A failure of both names both causes.
    ///
    /// On each leg, each door of [`door::doors`] in turn. When a front door
    /// shows a key that is not the pinned key, read the device record again
    /// and try each of its doors with its key, the door that showed the key
    /// first; a door that shows another key again is tried once more after a
    /// second. A failure of each door names the key that did not match. A
    /// key that the API did not give is never taken.
    pub async fn open_session(&self) -> Result<MasqueSession, Failure> {
        let settings = &self.settings;
        if let Some(peer) = &settings.wireguard {
            let session = MasqueSession::connect_wg(peer, &settings.way).await?;
            settings.report.decision("warp.leg", "wireguard");
            return Ok(session);
        }
        let Some(relay) = settings.relay.as_ref().filter(|_| !settings.way.h3) else {
            let session = self.open_on(&settings.way, settings.routes).await?;
            settings.report.decision("warp.leg", "no-relay");
            self.note_leg(Leg::Direct);
            return Ok(session);
        };
        let through_way = Way { dial: relay.dial.clone(), meter: relay.meter.clone(), ..settings.way.clone() };
        let relay_first = self.relay_first();
        let delay = relay.delay_for(relay_first);
        // The relay starts at its delay, or when the direct path failed.
        let go = tokio::sync::Notify::new();
        let direct = self.open_on(&settings.way, settings.routes);
        let through = async {
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = go.notified() => {}
            }
            // A relay reaches both families.
            self.open_on(&through_way, Routes::default()).await
        };
        tokio::pin!(direct);
        tokio::pin!(through);
        let mut direct_err: Option<Failure> = None;
        let mut relay_err: Option<Failure> = None;
        while direct_err.is_none() || relay_err.is_none() {
            tokio::select! {
                result = &mut direct, if direct_err.is_none() => match result {
                    Ok(session) => {
                        settings.report.decision("warp.leg", "direct");
                        self.note_leg(Leg::Direct);
                        return Ok(session);
                    }
                    Err(err) => {
                        if relay_err.is_none() {
                            let text = format!("the direct MASQUE path failed ({}); the relay {} is tried", err.chain(), relay.name);
                            settings.report.event(Event::warn("warp.leg", text));
                        }
                        go.notify_one();
                        direct_err = Some(err);
                    }
                },
                result = &mut through, if relay_err.is_none() => match result {
                    Ok(session) => {
                        tracing::info!(relay = %relay.name, "MASQUE goes through the relay");
                        settings.report.decision("warp.leg", if relay_first { "relay-first" } else { "relay" });
                        self.note_leg(Leg::Relay);
                        return Ok(session);
                    }
                    Err(err) => relay_err = Some(err),
                },
            }
        }
        let (Some(direct), Some(through)) = (direct_err, relay_err) else {
            unreachable!("the loop ends when both legs failed");
        };
        let (stage, class) = (direct.stage, direct.class);
        let (steps, cause) = through.into_root();
        let text = format!("{}; and through the relay {}: {steps}", direct.chain(), relay.name);
        Err(match cause {
            Some(cause) => Failure::caused(stage, class, text, cause),
            None => Failure::new(stage, class, text),
        })
    }

    /// Open a session on the way `way`: each door in turn.
    async fn open_on(&self, way: &Way, routes: Routes) -> Result<MasqueSession, Failure> {
        let settings = &self.settings;
        let pinned = self.pin()?;
        if self.doors().is_empty() {
            return Err(Failure::new(Stage::Warp, Class::Protocol, "the device has no MASQUE front door"));
        }
        let candidates = self.doors_of(routes)?;
        let mut failures: Vec<Failure> = Vec::new();
        for (i, door) in candidates.iter().enumerate() {
            match MasqueSession::connect(door, &self.identity, pinned.clone(), way).await {
                Ok(session) => {
                    self.note_won(door);
                    settings.report.decision("warp.door", if i == 0 { "first" } else { "other" });
                    return Ok(session);
                }
                Err(err) if is_pin_mismatch(&err) => return self.open_after_mismatch(err, way, door, routes).await,
                Err(err) => {
                    if let Some(next) = candidates.get(i + 1) {
                        let text = format!("the front door {door} failed ({}); {next} is tried", err.chain());
                        settings.report.event(Event::warn("warp.door", text));
                    }
                    failures.push(err);
                }
            }
        }
        Err(each_door_failed(failures))
    }

    /// The doors that a session tries, in order, without a door of a family
    /// that the host has no route to (PodUtils/podnet#366): each dropped
    /// door is a line of the report, and a set with no reachable door is a
    /// named failure before any socket.
    fn doors_of(&self, routes: Routes) -> Result<Vec<Target>, Failure> {
        self.doors_from(self.doors(), routes)
    }

    /// [`Warp::doors_of`] of the list `all`: the order of the pinned door
    /// or of [`door::doors`] on its addresses, then the doors of a name.
    fn doors_from(&self, all: Vec<Target>, routes: Routes) -> Result<Vec<Target>, Failure> {
        let bad = *self.bad.lock().unwrap_or_else(|e| e.into_inner());
        let addresses: Vec<std::net::SocketAddr> =
            all.iter().filter_map(|t| if let Target::Addr(a) = t { Some(*a) } else { None }).collect();
        let mut candidates = door::doors(&self.settings.door, addresses, self.won_door(), bad, routes);
        for named in all.iter().filter(|t| !matches!(t, Target::Addr(_))) {
            if !candidates.contains(named) {
                candidates.push(named.clone());
            }
        }
        let mut reachable = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if door::reachable(&candidate, routes) {
                reachable.push(candidate);
            } else {
                let text = format!(
                    "the front door {candidate} has no route on this host ({}); it is not tried",
                    door::NO_FAMILY
                );
                self.settings.report.event(Event::warn("warp.door", text));
            }
        }
        if reachable.is_empty() {
            let default = match &self.settings.door {
                Door::Pinned(target) => target.clone(),
                Door::Record => all.first().cloned().unwrap_or_else(|| Target::Addr(([0u8; 4], 0).into())),
            };
            let failure = door::unreachable(&all, routes, &default);
            self.settings.report.event(Event::warn("warp.door", failure.chain()));
            return Err(failure);
        }
        Ok(reachable)
    }

    fn note_won(&self, door: &Target) {
        if let Target::Addr(addr) = door {
            *self.won.lock().unwrap_or_else(|e| e.into_inner()) = Some(*addr);
        }
    }

    /// The front door `shown` showed another key than the pinned one (a new
    /// device, or a new key of the door): read the record again and try each
    /// of its doors, `shown` first when the record still lists it. A door
    /// that fails for another cause does not end the try: the next door can
    /// open. A door that shows another key again is
    /// tried once more after [`PIN_RETRY`].
    async fn open_after_mismatch(
        &self,
        err: Failure,
        way: &Way,
        shown: &Target,
        routes: Routes,
    ) -> Result<MasqueSession, Failure> {
        let settings = &self.settings;
        if self.device.id.is_empty() {
            let provider = self.provider().unwrap_or("the provider");
            let text =
                format!("the MASQUE key is not the pinned key, and {provider} keeps no device record to read again");
            return Err(Failure::caused(Stage::Warp, Class::Protocol, text, err));
        }
        let text = format!("the MASQUE key is not the pinned key ({}); the device record is read again", err.chain());
        settings.report.event(Event::warn("warp.pin", text));
        let fresh = register::get_device(&settings.api, &self.device.id, &self.device.token).await.map_err(|e| {
            let why = why_shown(way.dial.proxy_label(shown), &err);
            let text = format!("{}; reading the device record again failed. {why}", err.chain());
            Failure::caused(e.stage, e.class, text, e)
        })?;
        let pinned = match crate::provider::Enrolled::of_device("cloudflare", fresh.clone()).pin {
            pin if !pin.is_empty() => pin,
            _ => pin_of(&fresh)?,
        };
        // The doors of the record read again, not the doors of the
        // enrolment: the key that the door shows moved with its record.
        let fresh_doors: Vec<Target> = fresh.masque_endpoints().into_iter().map(Target::Addr).collect();
        let mut doors = self.doors_from(fresh_doors, routes)?;
        if let Some(i) = doors.iter().position(|d| d == shown) {
            let door = doors.remove(i);
            doors.insert(0, door);
        }
        if doors.is_empty() {
            let text =
                "the MASQUE key is not the pinned key, and the device record read again has no MASQUE front door";
            return Err(Failure::caused(Stage::Warp, Class::Protocol, text, err));
        }
        let mut failures: Vec<(Target, Failure)> = Vec::new();
        for (i, door) in doors.iter().enumerate() {
            match MasqueSession::connect(door, &self.identity, pinned.clone(), way).await {
                Ok(session) => return Ok(self.won_fresh(door, session)),
                Err(e) => {
                    if let Some(next) = doors.get(i + 1) {
                        let text = format!("the front door {door} failed ({}); {next} is tried", e.chain());
                        settings.report.event(Event::warn("warp.door", text));
                    }
                    failures.push((door.clone(), e));
                }
            }
        }
        // A new connection may reach another server.
        if failures.iter().any(|(_, e)| is_pin_mismatch(e)) {
            tokio::time::sleep(PIN_RETRY).await;
        }
        for (door, last) in failures.iter_mut().filter(|(_, e)| is_pin_mismatch(e)) {
            match MasqueSession::connect(door, &self.identity, pinned.clone(), way).await {
                Ok(session) => return Ok(self.won_fresh(door, session)),
                Err(e) => *last = e,
            }
        }
        Err(after_mismatch(&err, shown, failures, |door| way.dial.proxy_label(door)))
    }

    fn won_fresh(&self, door: &Target, session: MasqueSession) -> MasqueSession {
        self.note_won(door);
        self.settings.report.decision("warp.door", "fresh-record");
        session
    }
}

/// One failure of the doors that each failed, in order (none showed another
/// key): the text of each, and the typed cause of the last one as the
/// cause, so that a reader that joins the chain says each cause once. The
/// class is the one of the first door. The stage is `Proxy` when the proxy
/// on the way to any door failed ([`at_proxy`]): the chain keeps the root of
/// the last door alone, so a reader that looks for a failure of the proxy
/// in it (podssh's choice of HTTP/3) sees the proxy of each door here
/// (PodUtils/podssh#360, ask 2). Else it is the stage of the first door.
fn each_door_failed(failures: Vec<Failure>) -> Failure {
    let (first, class) = failures.first().map(|f| (f.stage, f.class)).expect("one door at least");
    let stage = if failures.iter().any(at_proxy) { Stage::Proxy } else { first };
    let n = failures.len();
    let mut text: Vec<String> = Vec::with_capacity(n);
    let mut cause = None;
    for (i, failure) in failures.into_iter().enumerate() {
        let words = if i + 1 < n {
            failure.chain()
        } else {
            let (steps, root) = failure.into_root();
            cause = root;
            steps
        };
        text.push(if i == 1 { format!("the other address of the front door failed too: {words}") } else { words });
    }
    let text = text.join("; ");
    match cause {
        Some(cause) => Failure::caused(stage, class, text, cause),
        None => Failure::new(stage, class, text),
    }
}

/// Whether the chain of `failure` holds a failure at the stage `Proxy`: the
/// proxy on the way to the door failed, not the door.
fn at_proxy(failure: &Failure) -> bool {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(failure);
    while let Some(e) = next {
        if e.downcast_ref::<Failure>().is_some_and(|f| f.stage == Stage::Proxy) {
            return true;
        }
        next = e.source();
    }
    false
}

/// One failure of the doors of the record read again after `err` (a key that
/// is not the pinned one, at the door `shown`). The source is the failure
/// of a door that shows another key, else of the first door; the text says
/// each other cause once (docs/design.md, section 1, rule 9): `err` only
/// when its cause is not the one of the source, and the cause of another
/// door only when it is not the one of `err` (PodUtils/podnet#726). When a
/// door shows another key still, the text says why ([`why_shown`]); `proxy`
/// names the proxy on the way to a door.
fn after_mismatch(
    err: &Failure,
    shown: &Target,
    mut failures: Vec<(Target, Failure)>,
    proxy: impl Fn(&Target) -> Option<String>,
) -> Failure {
    let which = failures.iter().position(|(_, e)| is_pin_mismatch(e));
    let (door, cause) = failures.swap_remove(which.unwrap_or(0));
    let first = cause_of(err, shown);
    let mut text =
        if cause_of(&cause, &door) == first { "the MASQUE key is not the pinned key".to_string() } else { err.chain() };
    text.push_str("; the device record was read again, and each of its doors failed");
    let mut others: Vec<String> = Vec::new();
    for (other, e) in &failures {
        let line = e.chain();
        if cause_of(e, other) != first && !others.contains(&line) {
            others.push(line);
        }
    }
    if !others.is_empty() {
        text.push_str(&format!(" ({})", others.join("; ")));
    }
    if which.is_some() {
        let why = why_shown(proxy(&door), &cause);
        text.push_str(&format!(". The key of the device record read again does not match either. {why}"));
    }
    Failure::caused(cause.stage, cause.class, text, cause)
}

/// The cause of the failure `e` of `door`, with no address of the door: two
/// doors that fail the same way give the same cause.
fn cause_of(e: &Failure, door: &Target) -> String {
    e.chain().replace(&door.to_string(), "")
}

/// Why a front door shows a certificate in place of WARP's key, by the
/// issuer of that certificate in `failure` (PodUtils/podnet#726): Let's
/// Encrypt and Cloudflare are WARP's side, and a later try passes; another
/// issuer is a middlebox that terminates TLS, and no root helps, because
/// the key is pinned. `proxy` is the proxy on the way to the door, which
/// can terminate TLS too: a line that blames a proxy that is not there
/// sends the user to look for it (PodUtils/podssh#234).
fn why_shown(proxy: Option<String>, failure: &Failure) -> String {
    let chain = failure.chain();
    let issuer = chain.split_once(crate::tls::SHOWN).map_or("", |(_, issuer)| issuer);
    let words = match Issuer::of(issuer) {
        Issuer::LetsEncrypt => {
            "WARP's side shows a certificate of Let's Encrypt at times in place of its key; \
                               a later try passes then"
        }
        Issuer::Warp => {
            "WARP's side shows such a certificate at times, while its API fails in the same minutes; \
                         a later try passes then"
        }
        Issuer::Middlebox => {
            return match proxy {
                Some(proxy) => format!(
                    "The proxy {proxy}, or a middlebox on the way, terminates TLS; the key is pinned, so no root helps: \
                     go around it"
                ),
                None => "A middlebox on the way terminates TLS; the key is pinned, so no root helps: go around it"
                    .to_string(),
            };
        }
    };
    match proxy {
        Some(proxy) => format!("{words}. Or the proxy {proxy} re-terminates TLS: then each try fails"),
        None => format!("{words}."),
    }
}

/// The class of the issuer of the certificate that a front door showed.
#[derive(Debug, PartialEq, Eq)]
enum Issuer {
    /// Let's Encrypt: WARP's side shows it at times.
    LetsEncrypt,
    /// Cloudflare, or no issuer known: WARP's side.
    Warp,
    /// Another issuer: a middlebox on the way.
    Middlebox,
}

impl Issuer {
    /// The class of `words`, the issuer as [`crate::der::issuer_words`]
    /// gives it (empty when the failure names none).
    fn of(words: &str) -> Issuer {
        let named = |org: &str| words.split(", ").any(|field| field.starts_with(&format!("O={org}")));
        if named("Let's Encrypt") {
            Issuer::LetsEncrypt
        } else if words.is_empty() || words.starts_with("an unnamed issuer") || named("Cloudflare") {
            Issuer::Warp
        } else {
            Issuer::Middlebox
        }
    }
}

#[cfg(test)]
mod tests;
