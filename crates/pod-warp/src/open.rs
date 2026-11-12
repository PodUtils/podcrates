//! The open of a MASQUE session: each door in turn, and the device record
//! read again when the front door shows another key than the pinned one.

use std::time::Duration;

use pod_core::{Class, Event, Failure, Stage, Target};

use crate::device::Device;
use crate::door;
use crate::identity::spki_from_pem;
use crate::register;
use crate::session::MasqueSession;
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
    /// Open a MASQUE session: each door of [`door::doors`] in turn, through
    /// the caller's dial. When the front door shows a key that is not the
    /// pinned key, read the device record again and try its door and its
    /// key, then once more after a second. A key that the API did not give
    /// is never taken.
    pub async fn open_session(&self) -> Result<MasqueSession, Failure> {
        let settings = &self.settings;
        let pinned = pin_of(&self.device)?;
        let candidates = door::doors(
            &settings.door,
            self.device.masque_endpoints(),
            self.won_door(),
            *self.bad.lock().unwrap_or_else(|e| e.into_inner()),
            settings.routes,
        );
        if candidates.is_empty() {
            return Err(Failure::new(Stage::Warp, Class::Protocol, "the device has no MASQUE front door"));
        }
        let mut failures: Vec<Failure> = Vec::new();
        for (i, door) in candidates.iter().enumerate() {
            match MasqueSession::connect(door, &self.identity, pinned.clone(), &settings.way).await {
                Ok(session) => {
                    self.note_won(door);
                    settings.report.decision("warp.door", if i == 0 { "first" } else { "other" });
                    return Ok(session);
                }
                Err(err) if is_pin_mismatch(&err) => return self.open_after_mismatch(err).await,
                Err(err) => {
                    if let Some(next) = candidates.get(i + 1) {
                        let text = format!("the front door {door} failed ({}); {next} is tried", err.chain());
                        settings.report.event(Event::warn("warp.door", text));
                    }
                    failures.push(err);
                }
            }
        }
        let mut failures = failures.into_iter();
        let first = failures.next().expect("one door at least");
        Err(failures.fold(first, |acc, e| {
            let text = format!("{}; the other address of the front door failed too ({})", acc.text(), e.chain());
            Failure::caused(acc.stage, acc.class, text, acc)
        }))
    }

    fn note_won(&self, door: &Target) {
        if let Target::Addr(addr) = door {
            *self.won.lock().unwrap_or_else(|e| e.into_inner()) = Some(*addr);
        }
    }

    /// The front door showed another key than the pinned one (a new device,
    /// or a new key of the door): read the record again and try its door.
    async fn open_after_mismatch(&self, err: Failure) -> Result<MasqueSession, Failure> {
        let settings = &self.settings;
        let text = format!("the MASQUE key is not the pinned key ({}); the device record is read again", err.chain());
        settings.report.event(Event::warn("warp.pin", text));
        let fresh = register::get_device(&settings.api, &self.device.id, &self.device.token).await.map_err(|e| {
            Failure::caused(e.stage, e.class, "reading the device record again after a key mismatch", e)
        })?;
        let pinned = pin_of(&fresh)?;
        let door = match &settings.door {
            door::Door::Pinned(target) => target.clone(),
            door::Door::Record => fresh.masque_endpoint().map(Target::Addr).ok_or_else(|| {
                Failure::new(Stage::Warp, Class::Protocol, "the device record has no MASQUE front door")
            })?,
        };
        let mut last = err;
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(PIN_RETRY).await;
            }
            match MasqueSession::connect(&door, &self.identity, pinned.clone(), &settings.way).await {
                Ok(session) => {
                    self.note_won(&door);
                    settings.report.decision("warp.door", "fresh-record");
                    return Ok(session);
                }
                Err(e) if attempt == 0 && is_pin_mismatch(&e) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}
