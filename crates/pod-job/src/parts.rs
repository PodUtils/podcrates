//! The optional parts of an offer that githome and podrunner added (T-923,
//! GitHub #223 G-16, G-20, G-36; PodUtils/podrunner#294): the cache of a
//! job and a sandbox payload. Each is optional, so `job-offer.v1` keeps its
//! id (the ruling "Schema ids").

use serde::{Deserialize, Serialize};

/// The cache of a job (G-20): one key, and how the unit may use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cache {
    /// The key of the cache, chosen by the source.
    pub key: String,
    pub mode: CacheMode,
}

/// How a unit may use the cache of its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
pub enum CacheMode {
    /// Read and write.
    #[serde(rename = "rw")]
    ReadWrite,
    /// Read only.
    #[serde(rename = "ro")]
    ReadOnly,
    /// No cache.
    #[serde(rename = "none")]
    Off,
}

/// A sandbox payload (G-36): an image by digest and a command, with the
/// bounds of its idle time and of its run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Sandbox {
    /// `NAME@sha256:HEX`.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::pinned_image"))]
    pub image: String,
    /// The command and its arguments; at least one word.
    #[cfg_attr(any(test, feature = "schema"), schemars(length(min = 1)))]
    pub command: Vec<String>,
    /// After this time with no input or output, the unit ends the sandbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_s: Option<u64>,
    /// After this time, the unit ends the sandbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_s: Option<u64>,
}

/// True for `NAME@sha256:` and 64 lower-case hex digits, with a name.
pub fn is_pinned_image(s: &str) -> bool {
    s.rsplit_once('@').is_some_and(|(name, digest)| !name.is_empty() && crate::offer::is_digest(digest))
}

/// True for a host name: letters, digits, `-` and `.`, in lower case, at
/// most 253 bytes, with no empty label.
pub fn is_host_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

impl Sandbox {
    /// The rules that serde does not check.
    pub fn check(&self) -> Result<(), String> {
        if !is_pinned_image(&self.image) {
            return Err(format!("the sandbox image {:?} is not NAME@sha256:HEX", self.image));
        }
        if self.command.is_empty() {
            return Err("the sandbox command is empty".into());
        }
        Ok(())
    }
}
