//! `unit-capability.v1`: what one unit kind of one host gives, as its
//! driver's `probe` reports it. A value that the driver cannot prove is
//! `unknown`, and placement reads `unknown` as absent. There is no value
//! `isolated`: the boundary names the mechanism.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The boundary between a job and the host, weakest first. `redirect`
/// rewrites file paths and is no isolation against hostile code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Boundary {
    None,
    Redirect,
    Confined,
    Guest,
}

/// Units and the control plane run as one uid (`shared`) or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Principal {
    Shared,
    Distinct,
    Unknown,
}

/// How a unit runs the containers of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ContainerRoute {
    /// No container: a job that needs one is refused
    /// (`container-not-supported`).
    None,
    /// The container hooks of the runner (call C2).
    Hooks,
    Unknown,
}

/// How a credential reaches the unit. There is no `env`: a credential never
/// goes into the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecretDelivery {
    Files,
    Socket,
}

/// Where the credential files live while the unit runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CredentialStore {
    /// A directory of mode 0700 on the disk of the host.
    PrivateDir,
    /// A memory file system.
    Tmpfs,
    Unknown,
}

/// The kill of the whole process tree of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TreeKill {
    /// Conformance test 3 passed on this host.
    Verified,
    Unverified,
    Unknown,
}

impl TreeKill {
    /// Only a verified tree kill can enforce a wall time.
    pub fn is_verified(self) -> bool {
        self == TreeKill::Verified
    }
}

/// How the network traffic of a unit reaches the egress broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum EgressHold {
    None,
    /// A redirect that a payload can go around.
    Advisory,
    Enforced,
    /// Read as `none`.
    Unknown,
}

impl EgressHold {
    /// The hold that placement may count on.
    pub fn effective(self) -> EgressHold {
        if self == EgressHold::Unknown { EgressHold::None } else { self }
    }
}

/// How the unit applies one limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LimitMode {
    Enforced,
    /// Measured and reported, not enforced.
    Reported,
    None,
}

/// A fact that is true, false or not known. `unknown` is read as false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Fact {
    Yes,
    No,
    Unknown,
}

impl Fact {
    pub fn holds(self) -> bool {
        self == Fact::Yes
    }
}

/// The capability record of one unit kind (`unit-capability.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub schema: u32,
    /// The unit kind: `fake`, `podvm`, `microvm`, `docker` or `host`.
    pub kind: String,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    pub boundary: Boundary,
    /// The name of the proof of the boundary (a self-test, a probe), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_proof: Option<String>,
    pub principal: Principal,
    pub container: ContainerRoute,
    /// The way out of the unit: `broker`, `none`, or `host`.
    pub net: String,
    pub secret_delivery: Vec<SecretDelivery>,
    pub credential_store: CredentialStore,
    /// The modes of a mount that the unit can give: `rw`, `ro`.
    pub mount_modes: Vec<String>,
    pub rootfs_fresh: Fact,
    pub tree_kill: TreeKill,
    /// One mode for each limit (`wall`, `cpu`, `memory`, `pids`, `disk`). A
    /// limit with no entry is `none`.
    #[serde(default)]
    pub limits: BTreeMap<String, LimitMode>,
    pub egress_hold: EgressHold,
    pub kvm: Fact,
}

impl Capability {
    /// The rules that serde does not check.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != crate::SCHEMA {
            return Err(format!("schema {} is not {}", self.schema, crate::SCHEMA));
        }
        if !matches!(self.arch.as_str(), "x86_64" | "aarch64") {
            return Err("arch is not x86_64 or aarch64".into());
        }
        Ok(())
    }

    /// The mode of one limit. A `wall` limit is enforced only with a
    /// verified tree kill.
    pub fn limit(&self, name: &str) -> LimitMode {
        let mode = self.limits.get(name).copied().unwrap_or(LimitMode::None);
        if name == "wall" && mode == LimitMode::Enforced && !self.tree_kill.is_verified() {
            return LimitMode::Reported;
        }
        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Capability {
        Capability {
            schema: 1,
            kind: "fake".into(),
            arch: "x86_64".into(),
            boundary: Boundary::Redirect,
            boundary_proof: None,
            principal: Principal::Shared,
            container: ContainerRoute::None,
            net: "broker".into(),
            secret_delivery: vec![SecretDelivery::Files],
            credential_store: CredentialStore::PrivateDir,
            mount_modes: vec!["rw".into()],
            rootfs_fresh: Fact::Yes,
            tree_kill: TreeKill::Unverified,
            limits: BTreeMap::from([("wall".into(), LimitMode::Enforced)]),
            egress_hold: EgressHold::Unknown,
            kvm: Fact::Unknown,
        }
    }

    #[test]
    fn boundaries_are_ordered() {
        assert!(Boundary::None < Boundary::Redirect);
        assert!(Boundary::Redirect < Boundary::Confined);
        assert!(Boundary::Confined < Boundary::Guest);
    }

    #[test]
    fn unknown_is_absent() {
        let c = sample();
        assert_eq!(c.egress_hold.effective(), EgressHold::None);
        assert!(!c.kvm.holds());
        // A wall limit needs a verified tree kill.
        assert_eq!(c.limit("wall"), LimitMode::Reported);
        assert_eq!(c.limit("memory"), LimitMode::None);
        let verified = Capability { tree_kill: TreeKill::Verified, ..c };
        assert_eq!(verified.limit("wall"), LimitMode::Enforced);
    }

    #[test]
    fn the_record_round_trips_and_refuses_isolated_and_env() {
        let c = sample();
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Capability>(&text).unwrap(), c);
        let isolated = text.replace("\"redirect\"", "\"isolated\"");
        assert!(serde_json::from_str::<Capability>(&isolated).is_err());
        let env = text.replace("[\"files\"]", "[\"env\"]");
        assert!(serde_json::from_str::<Capability>(&env).is_err());
        let extra = text.replacen('{', "{\"extra\":1,", 1);
        assert!(serde_json::from_str::<Capability>(&extra).is_err());
    }
}
