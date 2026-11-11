//! The records of the job of a CI runner (PodUtils/podrunner#17): the job
//! offer of a source, the capability record of a unit kind, the run report
//! and the public report, with their ids and the refusal codes. Each record
//! starts with `"schema": 1` ([`SCHEMA`]). serde reads the form of a record;
//! the `check` function of each record applies the other rules.
//!
//! The JSON Schemas of `schemas/` come from these types (ruling Q-A8): a
//! test writes them and compares them. The crate reads no environment, does
//! no I/O and has no pod dependency.

pub mod capability;
mod codes;
mod ids;
pub mod offer;
pub mod report;

#[cfg(any(test, feature = "schema"))]
mod json_schema;
#[cfg(test)]
mod schemas;

pub use capability::{
    Boundary, Capability, ContainerRoute, CredentialStore, EgressHold, Fact, LimitMode, Principal, SecretDelivery,
    TreeKill,
};
pub use codes::{RefusalCode, Tier};
pub use ids::{IdError, JOB_ID_MAX, JobId, UnitId, UnitIds};
pub use offer::JobOffer;
pub use report::{ExitReason, PublicReport, Residual, RunReport, UnitFields};

/// The version of each record. A reader refuses another value. The name of
/// each schema file and its `$id` carry the same number (`NAME.v1`).
pub const SCHEMA: u32 = 1;

/// The start of the `$id` of each schema of `schemas/`: the schema file
/// `NAME.vN.schema.json` on the branch `main` of podcrates.
pub const SCHEMAS: &str = "https://github.com/PodUtils/podcrates/blob/main/schemas/";

/// The architectures of a unit and of a job.
pub const ARCHES: [&str; 2] = ["x86_64", "aarch64"];
