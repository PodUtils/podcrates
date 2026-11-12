//! A lab: its names, its knobs and its layout. The prefix of the
//! namespaces is the consumer's (sand-net used `sn`).

use std::fmt;

use crate::knobs::{KnobError, Knobs};
use crate::layout::{Layout, Role};

/// The most characters of a prefix and of a name.
pub const MAX_PREFIX: usize = 8;
pub const MAX_NAME: usize = 24;

/// A lab that `Lab::new` refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabError {
    /// A prefix or a name that is empty, too long, or has a character
    /// other than `a-z`, `0-9` and an inner `-`.
    BadName {
        what: &'static str,
        value: String,
    },
    /// A state directory that is not absolute or has a control character.
    BadStateDir(String),
    Knobs(KnobError),
}

impl fmt::Display for LabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LabError::BadName { what, value } => {
                write!(f, "the {what} {value:?} is not 1 to its limit of a-z, 0-9 and inner '-'")
            }
            LabError::BadStateDir(dir) => write!(f, "the state directory {dir:?} is not an absolute path"),
            LabError::Knobs(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LabError {}

impl From<KnobError> for LabError {
    fn from(e: KnobError) -> LabError {
        LabError::Knobs(e)
    }
}

impl From<LabError> for pod_core::Failure {
    fn from(e: LabError) -> pod_core::Failure {
        pod_core::Failure::caused(pod_core::Stage::Host, pod_core::Class::Usage, e.to_string(), e)
    }
}

fn is_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One lab: a checked knob set under a prefix and a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lab {
    prefix: String,
    name: String,
    knobs: Knobs,
    layout: Layout,
}

impl Lab {
    pub fn new(prefix: &str, name: &str, knobs: Knobs) -> Result<Lab, LabError> {
        if !is_name(prefix, MAX_PREFIX) {
            return Err(LabError::BadName { what: "prefix", value: prefix.to_string() });
        }
        if !is_name(name, MAX_NAME) {
            return Err(LabError::BadName { what: "name", value: name.to_string() });
        }
        knobs.check()?;
        let layout = Layout::of(&knobs);
        Ok(Lab { prefix: prefix.to_string(), name: name.to_string(), knobs, layout })
    }

    pub fn knobs(&self) -> &Knobs {
        &self.knobs
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The network namespace of `role`: `PREFIX-NAME-g` and so on.
    pub fn namespace(&self, role: Role) -> String {
        format!("{}-{}-{}", self.prefix, self.name, role.letter())
    }

    /// The stem of the lab's files in the state directory.
    pub(crate) fn stem(&self) -> String {
        format!("{}-{}", self.prefix, self.name)
    }
}

/// What the commands of a lab need from the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// An absolute directory of the guest for the lab's files (tayga's
    /// configuration and pid file).
    pub state_dir: String,
    /// The group id of the probe program: the guest's `ping_group_range`
    /// is this one id, as the range must be contiguous (F20).
    pub ping_gid: u32,
}

impl Settings {
    pub fn check(&self) -> Result<(), LabError> {
        if !self.state_dir.starts_with('/') || self.state_dir.chars().any(char::is_control) {
            return Err(LabError::BadStateDir(self.state_dir.clone()));
        }
        Ok(())
    }
}
