//! The facts of a host for its sources (githome's G-17): what the host can
//! run, so that a source offers no job that the host must refuse, and shows
//! its hosts to an operator. podrunner sends them in `hello`.

use serde::{Deserialize, Serialize};

use crate::{Boundary, ContainerRoute};

/// The facts of one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HostFacts {
    /// The version of podrunner.
    pub podrunner: String,
    /// The version of podvm of the kinds, when one names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub podvm: Option<String>,
    /// The version of podnet of the kinds, when one names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub podnet: Option<String>,
    pub arch: String,
    /// `/dev/kvm` is there.
    pub kvm: bool,
    pub kinds: Vec<KindFacts>,
}

/// The facts of one unit kind of the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KindFacts {
    pub name: String,
    pub boundary: Boundary,
    pub container: ContainerRoute,
    /// The largest values that a unit of the kind gets: CPU in
    /// milli-cores, memory and disk in bytes, and processes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pids: Option<u64>,
}
