//! A registered WARP device with its MASQUE identity, ready to open a
//! session: podnet's `WarpClient` with the settings of the caller in place
//! of its environment, its state directory and its globals.

use std::net::SocketAddr;
use std::sync::Mutex;

use pod_core::{Class, Failure, Report, Stage, Target};

use crate::api::Api;
use crate::device::Device;
use crate::door::{Door, Routes};
use crate::hints::DeviceHints;
use crate::identity::Identity;
use crate::leg::{Leg, Legs, Relay};
use crate::net::NetSettings;
use crate::provider::{Cloudflare, Enrolled, Enrolment};
use crate::register;
use crate::session::Way;
use crate::store::{self, IdentityFile, Stored};
use crate::wg::Peer;

/// The MTU of the tunnel: usque's default, and the minimum of IPv6.
pub const MTU: usize = 1280;

/// The MTU of the stack of a carried device (PodUtils/podnet issues 154 and
/// 155): [`MTU`] of the outer stack, less the IPv4 and the UDP header (28),
/// the header and the tag of WireGuard (32), and its padding (at most 15).
pub const CARRY_MTU: usize = 1200;

/// The settings of a caller. The crate reads no variable and no file that
/// these do not name.
#[derive(Clone)]
pub struct Settings {
    /// The WARP API through the HTTP client of the caller.
    pub api: Api,
    /// How a session reaches the front door: the dial, the TLS provider,
    /// the TLS name, the `:authority` and the time limit.
    pub way: Way,
    /// The relay leg: a second dial that races the dial of `way`
    /// ([`Warp::open_session`]). None: the direct path alone. HTTP/3 and
    /// WireGuard take no relay: the relay carries TCP.
    pub relay: Option<Relay>,
    /// WireGuard on the UDP of `way` in place of MASQUE (PodUtils/podnet
    /// issue 152): the peer of a WireGuard device of WARP.
    pub wireguard: Option<Peer>,
    /// A second WireGuard device carried inside the first session
    /// (PodUtils/podnet issues 154 and 155): [`Warp::connect`] gives the
    /// stack of this one, with [`CARRY_MTU`].
    pub carry: Option<Peer>,
    /// The doors of the record, or one pinned door.
    pub door: Door,
    /// The default routes of the host (podnet: `has_default_route`; both
    /// through a proxy or a relay).
    pub routes: Routes,
    /// The hints of a new device.
    pub hints: DeviceHints,
    /// The events for the user and the observer of decisions.
    pub report: Report,
    /// The stack over a session: the MTU, the TCP settings, the counters,
    /// the check of the data path and the budget of a relay.
    pub net: NetSettings,
}

impl Settings {
    /// The settings of `api` and `way`, with no relay, MASQUE with no carry,
    /// the doors of the record, both routes, no hints, no report and the
    /// stack of [`NetSettings::default`] ([`MTU`], the check of the data
    /// path).
    pub fn new(api: Api, way: Way) -> Settings {
        Settings {
            api,
            way,
            relay: None,
            wireguard: None,
            carry: None,
            door: Door::Record,
            routes: Routes::default(),
            hints: DeviceHints::default(),
            report: Report::default(),
            net: NetSettings::default(),
        }
    }
}

/// A registered device with its MASQUE identity.
pub struct Warp {
    pub(crate) settings: Settings,
    pub(crate) device: Device,
    pub(crate) identity: Identity,
    /// The doors and the pinned key of the provider that enrolled the
    /// device: the adapter of an enrolment (PodUtils/podnet#486, QUIC-4).
    pub(crate) enrolled: Option<Enrolled>,
    /// The front door that opened the last session (PodUtils/podnet issue
    /// 124).
    pub(crate) won: Mutex<Option<SocketAddr>>,
    /// A front door whose session carried no packet (PodUtils/podnet issue
    /// 133): it goes last.
    pub(crate) bad: Mutex<Option<SocketAddr>>,
    /// The leg that worked last (PodUtils/podnet issue 126).
    pub(crate) legs: Mutex<Legs>,
}

impl Warp {
    /// A client of an enrolled device and its identity.
    pub fn from_parts(settings: Settings, device: Device, identity: Identity) -> Warp {
        let legs = Mutex::new(Legs::default());
        Warp { settings, device, identity, enrolled: None, won: Mutex::new(None), bad: Mutex::new(None), legs }
    }

    /// A client of the enrolment `enrolled` of a provider: its doors and
    /// its pinned key, with no registration call of another provider.
    pub fn from_enrolment(settings: Settings, enrolled: Enrolled, identity: Identity) -> Warp {
        let device = enrolled.device.clone().unwrap_or_else(Device::empty);
        let legs = Mutex::new(Legs::default());
        Warp {
            settings,
            device,
            identity,
            enrolled: Some(enrolled),
            won: Mutex::new(None),
            bad: Mutex::new(None),
            legs,
        }
    }

    /// A client of a stored device.
    pub fn from_stored(settings: Settings, stored: Stored) -> Warp {
        Warp::from_parts(settings, stored.device, stored.identity)
    }

    /// Register with the provider of the settings and enrol a new identity:
    /// the anonymous registration of Cloudflare, unless another provider is
    /// given to [`Warp::enrol`].
    ///
    /// The `POST` and the `PATCH` of the WARP API are the calls of the
    /// adapter of Cloudflare; the trait keeps an enrolment failure apart
    /// from the transport of a session.
    pub async fn register(settings: Settings) -> Result<Warp, Failure> {
        let provider = Cloudflare::new(settings.api.clone());
        Warp::enrol(settings, &provider).await
    }

    /// Enrol a new identity with `provider`, and give its doors and its
    /// pinned key: the adapter decides where the doors are and which key
    /// the door pins. A failure of the enrolment is a failure of this
    /// crate's type, with the class and the words of [`crate::EnrolError`].
    pub async fn enrol(settings: Settings, provider: &dyn Enrolment) -> Result<Warp, Failure> {
        let identity = Identity::generate()?;
        let enrolled = provider.enrol(&identity, &settings.hints).await.map_err(|e| e.failure())?;
        Ok(Warp::from_enrolment(settings, enrolled, identity))
    }

    /// Resume the identity of `file`, or register and save one there
    /// ([`store::register_or_load`]).
    pub async fn register_or_load(settings: Settings, file: &IdentityFile) -> Result<Warp, Failure> {
        let stored = store::register_or_load(&settings.api, &settings.hints, file).await?;
        Ok(Warp::from_stored(settings, stored))
    }

    /// Save the device and the identity to `path` (mode 0600).
    pub fn save(&self, path: &std::path::Path) -> Result<(), Failure> {
        store::save(path, &self.device, &self.identity)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The device record: the addresses and the pinned key of the peer. A
    /// provider that keeps no record leaves it empty ([`Device::empty`]),
    /// and the account calls of the WARP API are `Usage`.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// The name of the provider that enrolled the device, if an adapter
    /// did.
    pub fn provider(&self) -> Option<&str> {
        self.enrolled.as_ref().map(|e| e.provider.as_str())
    }

    /// The record that the account calls of the WARP API need: a provider
    /// that keeps none is `Usage`.
    fn record(&self) -> Result<&Device, Failure> {
        match self.device.id.is_empty() {
            true => {
                let provider = self.provider().unwrap_or("the provider");
                let text = format!("{provider} keeps no device record; the account calls of the WARP API need one");
                Err(Failure::new(Stage::Warp, Class::Usage, text))
            }
            false => Ok(&self.device),
        }
    }

    /// The identity that the front door sees.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Read the device record from the API again; keep the token.
    pub async fn refresh(&mut self) -> Result<(), Failure> {
        let token = self.device.token.clone();
        let provider = self.provider().unwrap_or("cloudflare").to_string();
        let mut device = register::get_device(&self.settings.api, &self.device.id, &token).await?;
        device.token = token;
        self.device = device;
        self.enrolled = Some(crate::provider::Enrolled::of_device(&provider, self.device.clone()));
        Ok(())
    }

    /// The account record, with the license key hidden.
    pub async fn account(&self) -> Result<serde_json::Value, Failure> {
        let device = self.record()?;
        register::get_account(&self.settings.api, &device.id, &device.token).await
    }

    /// Attach a WARP+ license key, then read the device again.
    pub async fn set_license(&mut self, license: &str) -> Result<serde_json::Value, Failure> {
        let device = self.record()?;
        let account = register::set_license(&self.settings.api, &device.id, &device.token, license).await?;
        self.refresh().await?;
        Ok(account)
    }

    /// Make a new MASQUE key and enrol it. The old key stops working.
    pub async fn rotate_key(&mut self) -> Result<(), Failure> {
        self.record()?;
        let identity = Identity::generate()?;
        let token = self.device.token.clone();
        let name = self.settings.hints.name.clone();
        let mut device =
            register::enrol_masque(&self.settings.api, &self.device.id, &token, &identity.spki_b64(), name).await?;
        device.token = token;
        self.device = device;
        self.identity = identity;
        Ok(())
    }

    /// Remove the device from the WARP service.
    pub async fn delete(&self) -> Result<(), Failure> {
        let device = self.record()?;
        register::delete_device(&self.settings.api, &device.id, &device.token).await
    }

    /// The front door that opened the last session.
    pub fn won_door(&self) -> Option<SocketAddr> {
        *self.won.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start with `door` first, as the caller kept it (a door file).
    pub fn with_won_door(self, door: Option<SocketAddr>) -> Warp {
        *self.won.lock().unwrap_or_else(|e| e.into_inner()) = door;
        self
    }

    /// The door of the last session carried no packet: the next session
    /// tries the other doors first (PodUtils/podnet issue 133).
    pub fn mark_door_bad(&self) {
        let won = self.won.lock().unwrap_or_else(|e| e.into_inner()).take();
        *self.bad.lock().unwrap_or_else(|e| e.into_inner()) = won;
    }

    /// Whether the relay worked last: its delay is then
    /// [`Relay::delay_known`].
    pub fn relay_first(&self) -> bool {
        self.legs.lock().unwrap_or_else(|e| e.into_inner()).relay_first
    }

    /// Start with the relay first, as the caller kept it (a leg file read
    /// with [`crate::leg::relay_first_from`]).
    pub fn with_relay_first(self, relay_first: bool) -> Warp {
        self.legs.lock().unwrap_or_else(|e| e.into_inner()).relay_first = relay_first;
        self
    }

    /// The leg of the last session.
    pub fn leg(&self) -> Option<Leg> {
        self.legs.lock().unwrap_or_else(|e| e.into_inner()).last
    }

    /// The leg to write in the caller's leg file ([`crate::leg::leg_text`]),
    /// once, after a session that changed [`Warp::relay_first`]: the relay
    /// opened when the direct path went first, or the direct path opened
    /// when the relay went first.
    pub fn take_leg_change(&self) -> Option<Leg> {
        self.legs.lock().unwrap_or_else(|e| e.into_inner()).change.take()
    }

    pub(crate) fn note_leg(&self, leg: Leg) {
        self.legs.lock().unwrap_or_else(|e| e.into_inner()).won(leg);
    }

    /// The front door of the last session: the door of the WireGuard peer,
    /// the pinned door, else the door that worked, else the first of the
    /// device record.
    pub fn endpoint_in_use(&self) -> Option<Target> {
        if let Some(peer) = &self.settings.wireguard {
            return Some(Target::Addr(peer.endpoint));
        }
        match &self.settings.door {
            Door::Pinned(target) => Some(target.clone()),
            Door::Record => self.won_door().map(Target::Addr).or_else(|| self.doors().first().cloned()),
        }
    }

    /// The doors of the provider that enrolled the device, or those of the
    /// record when no adapter enrolled it.
    pub fn doors(&self) -> Vec<Target> {
        match &self.enrolled {
            Some(enrolled) => enrolled.doors.clone(),
            None => self.device.masque_endpoints().into_iter().map(Target::Addr).collect(),
        }
    }

    /// The key of the front door that the client pins, as DER: that of the
    /// provider that enrolled the device, else the peer key of the record.
    pub(crate) fn pin(&self) -> Result<Vec<u8>, Failure> {
        match &self.enrolled {
            Some(enrolled) if !enrolled.pin.is_empty() => Ok(enrolled.pin.clone()),
            Some(enrolled) => {
                let text =
                    format!("the provider {} enrolled no pinned key; no session opens without one", enrolled.provider);
                Err(Failure::new(Stage::Warp, Class::Protocol, text))
            }
            None => crate::open::pin_of(&self.device),
        }
    }
}
