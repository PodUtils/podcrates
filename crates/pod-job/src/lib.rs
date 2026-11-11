//! The records that cross the seams of podrunner (GitHub #17): the ids, the
//! credential, and the capability record of a unit kind. Each object on a
//! wire starts with `"schema": 1` ([`SCHEMA`]).
//!
//! Ruling Q-A8: the JSON Schemas live in podcrates (`schemas/`), generated
//! from crate types. These types are the draft that podrunner hands to
//! podcrates; podrunner takes the podcrates types when they are released.

pub mod capability;
mod codes;
mod ids;
pub mod offer;
pub mod report;
mod secret;

pub use capability::{
    Boundary, Capability, ContainerRoute, CredentialStore, EgressHold, Fact, LimitMode, Principal, SecretDelivery,
    TreeKill,
};
pub use codes::{RefusalCode, Tier};
pub use ids::{IdError, JobId, UnitId, UnitIds};
pub use offer::JobOffer;
pub use report::{ExitReason, PublicReport, Residual, RunReport, UnitFields};
pub use secret::{Credential, CredentialFile, Secret};

/// The version of each record and of each wire. A reader refuses another
/// value.
pub const SCHEMA: u32 = 1;

#[cfg(test)]
mod schemas;
