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
use crate::register;
use crate::session::Way;
use crate::store::{self, IdentityFile, Stored};

/// The MTU of the tunnel: usque's default, and the minimum of IPv6.
pub const MTU: usize = 1280;

/// The settings of a caller. The crate reads no variable and no file that
/// these do not name.
#[derive(Clone)]
pub struct Settings {
    /// The WARP API through the HTTP client of the caller.
    pub api: Api,
    /// How a session reaches the front door: the dial, the TLS provider,
    /// the TLS name, the `:authority` and the time limit.
    pub way: Way,
    /// The doors of the record, or one pinned door.
    pub door: Door,
    /// The default routes of the host (podnet: `has_default_route`; both
    /// through a proxy or a relay).
    pub routes: Routes,
    /// The hints of a new device.
    pub hints: DeviceHints,
    /// The events for the user and the observer of decisions.
    pub report: Report,
    /// The MTU of the stack over the session.
    pub mtu: usize,
}

impl Settings {
    /// The settings of `api` and `way`, with the doors of the record, both
    /// routes, no hints, no report and [`MTU`].
    pub fn new(api: Api, way: Way) -> Settings {
        Settings {
            api,
            way,
            door: Door::Record,
            routes: Routes::default(),
            hints: DeviceHints::default(),
            report: Report::default(),
            mtu: MTU,
        }
    }
}

/// A registered device with its MASQUE identity.
pub struct Warp {
    pub(crate) settings: Settings,
    pub(crate) device: Device,
    pub(crate) identity: Identity,
    /// The front door that opened the last session (PodUtils/podnet issue
    /// 124).
    pub(crate) won: Mutex<Option<SocketAddr>>,
    /// A front door whose session carried no packet (PodUtils/podnet issue
    /// 133): it goes last.
    pub(crate) bad: Mutex<Option<SocketAddr>>,
}

impl Warp {
    /// A client of an enrolled device and its identity.
    pub fn from_parts(settings: Settings, device: Device, identity: Identity) -> Warp {
        Warp { settings, device, identity, won: Mutex::new(None), bad: Mutex::new(None) }
    }

    /// A client of a stored device.
    pub fn from_stored(settings: Settings, stored: Stored) -> Warp {
        Warp::from_parts(settings, stored.device, stored.identity)
    }

    /// Register anonymously and enrol a new MASQUE identity, with no file:
    /// a `POST` that makes the device, and a `PATCH` that gives it a P-256
    /// key and the tunnel type `masque`.
    pub async fn register(settings: Settings) -> Result<Warp, Failure> {
        let identity = Identity::generate()?;
        let api = &settings.api;
        let device = register::create_device(api, &settings.hints).await?;
        if device.token.is_empty() {
            return Err(Failure::new(Stage::Warp, Class::Protocol, "the registration answer carried no token"));
        }
        let name = settings.hints.name.clone();
        let mut enrolled = register::enrol_masque(api, &device.id, &device.token, &identity.spki_b64(), name).await?;
        // Only the registration answer has the token; the account calls and
        // the rotation of the key need it.
        enrolled.token = device.token;
        if enrolled.tunnel_type != "masque" {
            let text = format!("the API kept tunnel_type {:?} in place of masque", enrolled.tunnel_type);
            return Err(Failure::new(Stage::Warp, Class::Protocol, text));
        }
        Ok(Warp::from_parts(settings, enrolled, identity))
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

    /// The device record: the addresses and the pinned key of the peer.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// The identity that the front door sees.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Read the device record from the API again; keep the token.
    pub async fn refresh(&mut self) -> Result<(), Failure> {
        let token = self.device.token.clone();
        let mut device = register::get_device(&self.settings.api, &self.device.id, &token).await?;
        device.token = token;
        self.device = device;
        Ok(())
    }

    /// The account record, with the license key hidden.
    pub async fn account(&self) -> Result<serde_json::Value, Failure> {
        register::get_account(&self.settings.api, &self.device.id, &self.device.token).await
    }

    /// Attach a WARP+ license key, then read the device again.
    pub async fn set_license(&mut self, license: &str) -> Result<serde_json::Value, Failure> {
        let account = register::set_license(&self.settings.api, &self.device.id, &self.device.token, license).await?;
        self.refresh().await?;
        Ok(account)
    }

    /// Make a new MASQUE key and enrol it. The old key stops working.
    pub async fn rotate_key(&mut self) -> Result<(), Failure> {
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
        register::delete_device(&self.settings.api, &self.device.id, &self.device.token).await
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

    /// The front door of the last session: the pinned door, else the door
    /// that worked, else the first of the device record.
    pub fn endpoint_in_use(&self) -> Option<Target> {
        match &self.settings.door {
            Door::Pinned(target) => Some(target.clone()),
            Door::Record => self.won_door().or_else(|| self.device.masque_endpoint()).map(Target::Addr),
        }
    }
}
