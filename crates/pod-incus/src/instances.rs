//! The instances (`shared/api/instance.go`, `instance_state.go`): create
//! from an image, change the state, list with `recursion=1`, get one (with
//! its ETag), change its config on the condition of that ETag, delete, and
//! the console log. Each call that incus runs in the background (create, a
//! state change, delete) returns after the wait for its operation.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::de::IgnoredAny;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::client::Client;
use crate::envelope::nullable;
use crate::error::{Error, Result};
use crate::operation::Operation;
use crate::wire::{Body, escape};

/// The devices of an instance: by name, the keys of each device.
pub type Devices = BTreeMap<String, BTreeMap<String, String>>;

/// An instance as incus lists it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Instance {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `Running`, `Stopped`, `Frozen`, `Error`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: u16,
    /// `container` or `virtual-machine`.
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_used_at: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default)]
    pub stateful: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub profiles: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub config: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "nullable")]
    pub devices: Devices,
    #[serde(default, deserialize_with = "nullable")]
    pub expanded_config: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "nullable")]
    pub expanded_devices: Devices,
}

/// The kind of an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    #[serde(rename = "container")]
    Container,
    #[serde(rename = "virtual-machine")]
    VirtualMachine,
}

/// The source of a new instance: an image of the server by alias or by
/// fingerprint, or none (an empty instance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Alias(String),
    Fingerprint(String),
    None,
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self {
            Source::Alias(alias) => {
                map.serialize_entry("type", "image")?;
                map.serialize_entry("alias", alias)?;
            }
            Source::Fingerprint(fingerprint) => {
                map.serialize_entry("type", "image")?;
                map.serialize_entry("fingerprint", fingerprint)?;
            }
            Source::None => map.serialize_entry("type", "none")?,
        }
        map.end()
    }
}

/// A new instance (`InstancesPost`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewInstance {
    pub name: String,
    pub source: Source,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub config: BTreeMap<String, String>,
    pub devices: Devices,
    /// `None`: the default profile of the project; an empty list: none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<String>>,
    pub ephemeral: bool,
    /// Start the instance after its creation.
    pub start: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl NewInstance {
    /// A container `name` from `source`, with no configuration and the
    /// default profile, not started.
    pub fn container(name: impl Into<String>, source: Source) -> NewInstance {
        NewInstance {
            name: name.into(),
            source,
            kind: Kind::Container,
            config: BTreeMap::new(),
            devices: Devices::new(),
            profiles: None,
            ephemeral: false,
            start: false,
            description: String::new(),
        }
    }
}

/// A change of the state (`InstanceStatePut`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    pub action: Action,
    /// For a stop and a restart: the time of a clean shutdown; after it,
    /// incus stops the instance. `None`: the default of incus (600 s).
    pub timeout: Option<Duration>,
    /// Stop or restart at once, with no clean shutdown.
    pub force: bool,
    /// Keep the state of the memory (CRIU, or the state of a VM).
    pub stateful: bool,
}

/// The action of a [`StateChange`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Freeze,
    Unfreeze,
}

impl StateChange {
    pub fn start() -> StateChange {
        StateChange { action: Action::Start, timeout: None, force: false, stateful: false }
    }

    /// A stop: a clean shutdown within `timeout`, or at once with `force`.
    pub fn stop(force: bool, timeout: Option<Duration>) -> StateChange {
        StateChange { action: Action::Stop, timeout, force, stateful: false }
    }
}

impl Serialize for StateChange {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(4))?;
        map.serialize_entry("action", &self.action)?;
        // -1 is no timeout given; 0 would be a stop at once.
        let timeout = self.timeout.map_or(-1, |t| i64::try_from(t.as_secs().max(1)).unwrap_or(i64::MAX));
        map.serialize_entry("timeout", &timeout)?;
        map.serialize_entry("force", &self.force)?;
        map.serialize_entry("stateful", &self.stateful)?;
        map.end()
    }
}

/// The ETag of an instance, as incus gives it in the `ETag` header of
/// `GET /1.0/instances/NAME` (with its quotes). The value is opaque: incus
/// makes it from the architecture, the local config (with its `volatile.*`
/// keys), the local devices, `ephemeral` and the profiles. A change of one
/// of them changes the ETag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ETag(String);

impl ETag {
    /// The ETag `value`, as incus gave it.
    pub fn new(value: impl Into<String>) -> ETag {
        ETag(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ETag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The body of a PATCH of the config alone (`InstancePut` with only
/// `config`): incus keeps each field that the body does not have.
#[derive(Serialize)]
struct ConfigPatch<'a> {
    config: &'a BTreeMap<String, String>,
}

fn instance_path(name: &str) -> String {
    format!("/1.0/instances/{}", escape(name))
}

impl Client {
    /// `POST /1.0/instances`: create the instance, and wait.
    pub async fn create_instance(&self, new: &NewInstance) -> Result<Operation> {
        self.run_json("POST", "/1.0/instances", new).await
    }

    /// `PUT /1.0/instances/NAME/state`: change the state, and wait.
    pub async fn set_state(&self, name: &str, change: &StateChange) -> Result<Operation> {
        self.run_json("PUT", &format!("{}/state", instance_path(name)), change).await
    }

    /// Start the instance.
    pub async fn start(&self, name: &str) -> Result<Operation> {
        self.set_state(name, &StateChange::start()).await
    }

    /// Stop the instance: see [`StateChange::stop`].
    pub async fn stop(&self, name: &str, force: bool, timeout: Option<Duration>) -> Result<Operation> {
        self.set_state(name, &StateChange::stop(force, timeout)).await
    }

    /// `GET /1.0/instances?recursion=1`: each instance of the project.
    pub async fn instances(&self) -> Result<Vec<Instance>> {
        self.get("/1.0/instances", &[("recursion", "1")]).await
    }

    /// `GET /1.0/instances/NAME`.
    pub async fn instance(&self, name: &str) -> Result<Instance> {
        self.get(&instance_path(name), &[]).await
    }

    /// `GET /1.0/instances/NAME`: the instance and its ETag, for a later
    /// [`Client::patch_instance_config`]. `None`: incus sent no ETag.
    pub async fn instance_with_etag(&self, name: &str) -> Result<(Instance, Option<ETag>)> {
        let (instance, etag) = self.get_tagged(&instance_path(name), &[]).await?;
        Ok((instance, etag.map(ETag)))
    }

    /// `PATCH /1.0/instances/NAME` with `{"config": config}`: incus sets
    /// each key of `config` in the local config of the instance and keeps
    /// each other key, the devices and the profiles; it applies the change
    /// to a running instance at once. With `if_match`, incus makes the
    /// change only while the ETag of the instance is that ETag
    /// (`If-Match`); else the error is [`ErrorKind::EtagMismatch`]
    /// (412). Another refusal of incus is [`ErrorKind::Incus`], with its
    /// class.
    ///
    /// [`ErrorKind::EtagMismatch`]: crate::ErrorKind::EtagMismatch
    /// [`ErrorKind::Incus`]: crate::ErrorKind::Incus
    pub async fn patch_instance_config(
        &self,
        name: &str,
        config: &BTreeMap<String, String>,
        if_match: Option<&ETag>,
    ) -> Result<()> {
        let headers = if_match.map(|etag| vec![("If-Match".to_string(), etag.0.clone())]).unwrap_or_default();
        let body = ConfigPatch { config };
        let sent = self.send_json::<IgnoredAny>("PATCH", &instance_path(name), headers, &body).await;
        sent.map(drop).map_err(Error::of_condition)
    }

    /// `DELETE /1.0/instances/NAME`: delete the instance, and wait. incus
    /// refuses a running instance.
    pub async fn delete_instance(&self, name: &str) -> Result<Operation> {
        self.run("DELETE", &instance_path(name), Vec::new(), Body::Empty).await
    }

    /// `GET /1.0/instances/NAME/console`: the console log of a container.
    pub async fn console_log(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self.raw(&format!("{}/console", instance_path(name)), &[]).await?.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_instance_has_the_fields_of_instances_post() {
        let mut new = NewInstance::container("web", Source::Alias("podvm/alpine".into()));
        new.config.insert("raw.lxc".into(), "lxc.init.cmd = /sbin/init".into());
        let json = serde_json::to_value(&new).unwrap();
        assert_eq!(json["source"], serde_json::json!({"type": "image", "alias": "podvm/alpine"}));
        assert_eq!((json["type"].as_str(), json["start"].as_bool()), (Some("container"), Some(false)));
        assert!(json.get("profiles").is_none() && json.get("description").is_none(), "{json}");
        new.profiles = Some(Vec::new());
        new.source = Source::Fingerprint("ab12".into());
        let json = serde_json::to_value(&new).unwrap();
        assert_eq!(json["profiles"], serde_json::json!([]));
        assert_eq!(json["source"], serde_json::json!({"type": "image", "fingerprint": "ab12"}));
        assert_eq!(serde_json::to_value(Source::None).unwrap(), serde_json::json!({"type": "none"}));
    }

    #[test]
    fn a_config_patch_has_the_config_alone() {
        let config: BTreeMap<String, String> = [("limits.memory".to_string(), "1GiB".to_string())].into();
        let json = serde_json::to_value(ConfigPatch { config: &config }).unwrap();
        assert_eq!(json, serde_json::json!({"config": {"limits.memory": "1GiB"}}));
    }

    #[test]
    fn a_state_change_gives_its_timeout_in_seconds_or_none() {
        let json = |c: StateChange| serde_json::to_value(c).unwrap();
        assert_eq!(
            json(StateChange::start()),
            serde_json::json!({"action": "start", "timeout": -1, "force": false, "stateful": false})
        );
        let stop = json(StateChange::stop(false, Some(Duration::from_secs(30))));
        assert_eq!((stop["action"].as_str(), stop["timeout"].as_i64()), (Some("stop"), Some(30)));
        // Less than one second is one second: 0 would stop at once.
        assert_eq!(json(StateChange::stop(false, Some(Duration::from_millis(10))))["timeout"], 1);
        assert_eq!(json(StateChange::stop(true, None))["force"], true);
    }
}
