//! The stored WARP identity and its registration (PodUtils/podnet@T-241).
//!
//! A registration is two remote calls: `POST /reg` creates the device, and
//! `PATCH /reg/{id}` enrols the MASQUE key. The service counts the device
//! after the first call. Thus:
//!
//! * a lock (`PATH.lock`) holds the whole sequence, so that two processes
//!   make one device;
//! * the device, its token and its key are saved with the state `created`
//!   as soon as the first call answers, and a later run sends the `PATCH`
//!   for the same device with no second `POST`;
//! * the complete record (no `state`) is saved after the `PATCH`.
//!
//! The file is podnet's `warp.json`: `device`, `cert_pem`, `key_pem` and
//! `state`. The path is the caller's.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use serde::{Deserialize, Serialize};

use crate::api::{Api, api_status};
use crate::device::Device;
use crate::files::{self, Foreign};
use crate::hints::DeviceHints;
use crate::identity::Identity;
use crate::register;

/// The state of a record that holds a created device with no MASQUE key.
const CREATED: &str = "created";
/// How long a process waits for the lock of another process.
pub const LOCK_WAIT: Duration = Duration::from_secs(60);

/// The identity file of a caller.
#[derive(Clone, Debug)]
pub struct IdentityFile {
    pub path: PathBuf,
    /// The consent to read a file of another user ([`Foreign`]).
    pub foreign: Foreign,
    /// The longest wait for the lock of another process.
    pub lock_wait: Duration,
}

impl IdentityFile {
    /// The file at `path`, of this user, with the wait [`LOCK_WAIT`].
    pub fn new(path: impl Into<PathBuf>) -> IdentityFile {
        IdentityFile { path: path.into(), foreign: Foreign::Refuse, lock_wait: LOCK_WAIT }
    }
}

/// A stored device and its identity.
#[derive(Clone, Debug)]
pub struct Stored {
    pub device: Device,
    pub identity: Identity,
    /// The file belongs to another user (read with [`Foreign::Allow`]): the
    /// crate writes nothing to it.
    pub foreign: bool,
}

#[derive(Serialize)]
struct Write<'a> {
    device: &'a Device,
    cert_pem: String,
    key_pem: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a str>,
}

#[derive(Deserialize)]
struct Read {
    device: Device,
    cert_pem: String,
    key_pem: String,
    #[serde(default)]
    state: Option<String>,
}

fn config(text: String) -> Failure {
    Failure::new(Stage::Warp, Class::Config, text)
}

/// Write a record durably (mode 0600). `state` is `None` for a complete
/// record.
fn write(path: &Path, device: &Device, identity: &Identity, state: Option<&str>) -> Result<(), Failure> {
    let (cert_pem, key_pem) = identity.to_pem();
    let text = serde_json::to_string_pretty(&Write { device, cert_pem, key_pem: &key_pem, state })
        .map_err(|e| Failure::caused(Stage::Warp, Class::Config, "serialising the WARP identity", e))?;
    let text = zeroize::Zeroizing::new(text);
    files::write_private(path, text.as_bytes()).map_err(|e| {
        let text = format!("writing the WARP identity to {}", path.display());
        Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
    })
}

/// Write a complete record.
pub fn save(path: &Path, device: &Device, identity: &Identity) -> Result<(), Failure> {
    write(path, device, identity, None)
}

/// Read a record: the device, the identity, the state, and whether the
/// file is of another user.
fn read(file: &IdentityFile) -> Result<(Stored, Option<String>), Failure> {
    let path = &file.path;
    let foreign = files::check_private(path, file.foreign)?;
    let text = std::fs::read_to_string(path).map_err(|e| {
        let text = format!("reading the WARP identity from {}", path.display());
        Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
    })?;
    let text = zeroize::Zeroizing::new(text);
    let Read { device, cert_pem, key_pem, state } = serde_json::from_str(&text)
        .map_err(|e| config(format!("the WARP identity in {} does not parse: {e}", path.display())))?;
    let key_pem = zeroize::Zeroizing::new(key_pem);
    if let Some(state) = state.as_deref().filter(|s| *s != CREATED) {
        return Err(config(format!("the WARP identity in {} has the unknown state {state:?}", path.display())));
    }
    let identity = Identity::from_pem(&cert_pem, &key_pem)?;
    Ok((Stored { device, identity, foreign }, state))
}

/// Read a complete record. A record with the state `created` is refused.
pub fn load(file: &IdentityFile) -> Result<Stored, Failure> {
    let (stored, state) = read(file)?;
    if state.is_some() {
        let text = format!(
            "the WARP device in {} is created and not enrolled; register again to finish it",
            file.path.display()
        );
        return Err(config(text));
    }
    Ok(stored)
}

/// True when `path` exists. A stat error is an error, not a missing file
/// (PodUtils/podnet issue 178).
fn exists(path: &Path) -> Result<bool, Failure> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => {
            let text = format!("reading the WARP identity from {}", path.display());
            Err(Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e))
        }
    }
}

/// Make sure that a file can be written at `path` before a registration: a
/// failed save after it spends a device.
fn check_writable(path: &Path) -> Result<(), Failure> {
    let mut probe = path.as_os_str().to_owned();
    probe.push(".probe");
    let probe = PathBuf::from(probe);
    let result = files::write_private(&probe, b"");
    let _ = std::fs::remove_file(&probe);
    result.map_err(|e| {
        let text = format!("the WARP identity cannot be written to {}", path.display());
        Failure::caused(Stage::Warp, Class::of_io(e.kind()), text, e)
    })
}

/// Register when the file holds nothing, else resume what it holds: one
/// device for each installation, not one for each start. A file that cannot
/// be read, or that other users can read, is a failure: nothing writes
/// over it. A file of another user (with consent) is read and never
/// written: a record in the state `created` there is a failure.
pub async fn register_or_load(api: &Api, hints: &DeviceHints, file: &IdentityFile) -> Result<Stored, Failure> {
    let path = &file.path;
    if file.foreign == Foreign::Allow && exists(path)? {
        let (stored, state) = read(file)?;
        if stored.foreign {
            if state.is_some() {
                let text = format!("the WARP device in {} of another user is not enrolled", path.display());
                return Err(config(text));
            }
            return Ok(stored);
        }
    }
    let _held = files::lock(path, file.lock_wait).await?;
    if exists(path)? {
        let (stored, state) = read(file)?;
        if state.is_some() {
            tracing::info!("the stored WARP device is not enrolled; it is finished");
            match finish(api, hints, path, stored.device, stored.identity, false).await {
                Err(err) if gone(&err) => {
                    tracing::info!("the service has no such device; a new one is registered");
                    std::fs::remove_file(path).map_err(|e| {
                        Failure::caused(Stage::Warp, Class::of_io(e.kind()), format!("removing {}", path.display()), e)
                    })?;
                }
                other => return other,
            }
        } else if stored.device.tunnel_type == "masque" {
            return Ok(stored);
        } else {
            tracing::debug!("the stored device is not enrolled for MASQUE; a new one is registered");
        }
    }
    check_writable(path)?;
    let identity = Identity::generate()?;
    let device = register::create_device(api, hints).await?;
    if device.token.is_empty() {
        return Err(Failure::new(Stage::Warp, Class::Protocol, "the registration answer carried no token"));
    }
    write(path, &device, &identity, Some(CREATED))?;
    finish(api, hints, path, device, identity, true).await
}

/// The service answered 401 or 404: it has no such device.
fn gone(err: &Failure) -> bool {
    matches!(api_status(err), Some(401 | 404))
}

/// Send the `PATCH` of a created device and save the complete record.
/// `fresh` is true for a device that this call made.
async fn finish(
    api: &Api,
    hints: &DeviceHints,
    path: &Path,
    device: Device,
    identity: Identity,
    fresh: bool,
) -> Result<Stored, Failure> {
    let token = device.token.clone();
    let enrolled = register::enrol_masque(api, &device.id, &token, &identity.spki_b64(), hints.name.clone()).await;
    let mut enrolled = match enrolled {
        Ok(enrolled) => enrolled,
        Err(err) => {
            // An answer of the service is final. No answer (a network
            // failure) leaves the record, and the next run resumes.
            if fresh && api_status(&err).is_some() {
                match register::delete_device(api, &device.id, &token).await {
                    Err(del) => tracing::warn!(error = %del, "the new WARP device is not deleted"),
                    Ok(()) => drop(std::fs::remove_file(path)),
                }
            }
            return Err(Failure::caused(err.stage, err.class, "enrolling the MASQUE identity", err));
        }
    };
    enrolled.token = token;
    if enrolled.tunnel_type != "masque" {
        let text = format!("the API kept tunnel_type {:?} in place of masque", enrolled.tunnel_type);
        return Err(Failure::new(Stage::Warp, Class::Protocol, text));
    }
    save(path, &enrolled, &identity)?;
    Ok(Stored { device: enrolled, identity, foreign: false })
}
