//! The report of one run: a versioned JSON object of [`Findings`] and
//! [`Selection`], and its reader (feature `report`).
//!
//! [`write()`] gives the object, and [`read()`] gives the findings and the
//! selection back. `write`, then `read`, then `write` gives the same text. A
//! caller that writes a document of its own puts this object under one key
//! and reads that key back with [`read`]: so the caller keeps one writer.
//!
//! The object holds what the run measured. It holds no constant of the crate
//! and no value that another field gives: the name of an errno
//! ([`crate::sys::Errno::name`]), the group of a row, the requirement of a
//! rung ([`Rung::requirement`]). `README.md` gives the fields.
//!
//! Each conversion from a type of the crate destructures the type with no
//! `..`: a new field does not compile until the object carries it.
//!
//! The feature `schema` gives [`schema()`], the JSON Schema of the object,
//! generated from the wire types; `schemas/hostprobe-report.v1.schema.json`
//! of the repository is that schema ([`SCHEMA_ID`]).

use serde::{Deserialize, Serialize};

use crate::Findings;
use crate::identity::Identity;
use crate::probes;
use crate::select::{Rejection, Rung, Selection};
use crate::sys::Errno;
use crate::verdict::{Outcome, Verdict};
use crate::writable::{Space, WriteProbe};

/// The version of the object: the value of its first field, `schema`.
/// [`read`] refuses each other value.
pub const SCHEMA: u32 = 1;

/// Why [`read`] refused a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The text is not a JSON object, or the object does not have the
    /// fields of its version.
    Form(String),
    /// The field `schema` is absent, is not a number, or names a version
    /// that this crate does not read. `None`: absent or not a number.
    Version(Option<u64>),
    /// A row whose name [`probes::PROBES`] does not name.
    UnknownRow(String),
    /// A row whose name comes two times.
    DuplicateRow(String),
    /// A word that the field does not take (`field` is `verdict` or `rung`).
    Word { field: &'static str, word: String },
    /// A denial with no errno: the name of the row or the path of the write
    /// probe. A denial with no errno is not a measurement.
    DenialWithoutErrno(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Form(why) => write!(f, "the report has not the form of schema {SCHEMA}: {why}"),
            ReadError::Version(Some(v)) => write!(f, "the report has schema {v}; this crate reads schema {SCHEMA}"),
            ReadError::Version(None) => write!(f, "the report has no numeric schema; this crate reads schema {SCHEMA}"),
            ReadError::UnknownRow(name) => write!(f, "the report has the row {name:?}, which no probe names"),
            ReadError::DuplicateRow(name) => write!(f, "the report has the row {name:?} two times"),
            ReadError::Word { field, word } => write!(f, "the report has {field} {word:?}, which is not a {field}"),
            ReadError::DenialWithoutErrno(at) => write!(f, "the report has a denial with no errno at {at:?}"),
        }
    }
}

impl std::error::Error for ReadError {}

// The wire form. The order of the fields is the order of the text.

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct Document {
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::version"))]
    schema: u32,
    findings: WireFindings,
    selection: WireSelection,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct WireFindings {
    rows: Vec<WireRow>,
    identity: WireIdentity,
    writable: Vec<WireWrite>,
    self_exe: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(
    any(test, feature = "schema"),
    schemars(extend(
        "if" = { "properties": { "verdict": { "const": "denied" } } },
        "then" = { "required": ["errno"], "properties": { "errno": { "type": "integer" } } }
    ))
)]
#[serde(deny_unknown_fields)]
struct WireRow {
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::row_name"))]
    name: String,
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::verdict"))]
    verdict: String,
    errno: Option<i32>,
    reason: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct WireIdentity {
    uid: i64,
    gid: i64,
    groups: Vec<i32>,
    uid_map: Option<String>,
    gid_map: Option<String>,
    setgroups: Option<String>,
    cap_eff: Option<String>,
    cap_prm: Option<String>,
    cap_bnd: Option<String>,
    seccomp: Option<String>,
    seccomp_filters: Option<String>,
    no_new_privs: Option<String>,
    threads: Option<String>,
    unreadable: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(
    any(test, feature = "schema"),
    schemars(extend(
        "if" = { "properties": { "verdict": { "const": "denied" } } },
        "then" = { "required": ["errno"], "properties": { "errno": { "type": "integer" } } }
    ))
)]
#[serde(deny_unknown_fields)]
struct WireWrite {
    path: String,
    source: String,
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::verdict"))]
    verdict: String,
    errno: Option<i32>,
    reason: String,
    space: Option<WireSpace>,
    space_errno: Option<i32>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct WireSpace {
    blocks_free: u64,
    block_size: i64,
    inodes_free: u64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct WireSelection {
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::rung"))]
    rung: String,
    rejected: Vec<WireRejection>,
    controls_answered: bool,
    control_notes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct WireRejection {
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "schema::rung"))]
    rung: String,
    evidence: String,
}

/// Each verdict, for the map from its word back to the verdict.
const VERDICTS: [Verdict; 3] = [Verdict::Ok, Verdict::Denied, Verdict::Skip];

/// Each rung, for the map from its word back to the rung.
const RUNGS: [Rung; 6] =
    [Rung::Namespace, Rung::Supervise, Rung::Chroot, Rung::Userland, Rung::Interpose, Rung::Unsupported];

/// A new variant must go into [`VERDICTS`] or [`RUNGS`]: these matches have
/// no wildcard, so a new variant stops the build here.
const _: fn(Verdict, Rung) = |v, r| {
    match v {
        Verdict::Ok | Verdict::Denied | Verdict::Skip => {}
    }
    match r {
        Rung::Namespace | Rung::Supervise | Rung::Chroot | Rung::Userland | Rung::Interpose | Rung::Unsupported => {}
    }
};

/// The object of `findings` and `selection`: one line of JSON, with no
/// newline at the end.
pub fn write(findings: &Findings, selection: &Selection) -> String {
    let Findings { rows, identity, writable, self_exe } = findings;
    let Selection { rung, rejected, controls_answered, control_notes } = selection;
    let document = Document {
        schema: SCHEMA,
        findings: WireFindings {
            rows: rows.iter().map(|(name, outcome)| wire_row(name, outcome)).collect(),
            identity: wire_identity(identity),
            writable: writable.iter().map(wire_write).collect(),
            self_exe: self_exe.clone(),
        },
        selection: WireSelection {
            rung: rung.word().to_string(),
            rejected: rejected
                .iter()
                .map(|Rejection { rung, requirement: _, evidence }| WireRejection {
                    rung: rung.word().to_string(),
                    evidence: evidence.clone(),
                })
                .collect(),
            controls_answered: *controls_answered,
            control_notes: control_notes.clone(),
        },
    };
    // The wire types hold strings, numbers, booleans and lists only, and
    // their maps have string keys: serde_json cannot fail on them.
    serde_json::to_string(&document).expect("the report serializes")
}

/// The findings and the selection of an object of [`write()`]. The order of
/// the keys and the white space are free. Each row name maps back to its
/// entry of [`probes::PROBES`].
pub fn read(document: &str) -> Result<(Findings, Selection), ReadError> {
    let value: serde_json::Value = serde_json::from_str(document).map_err(|e| ReadError::Form(e.to_string()))?;
    // A serde struct also reads a JSON array; the object is an object only.
    if !value.is_object() {
        return Err(ReadError::Form("the report is not a JSON object".to_string()));
    }
    // The version first, so that an object of another version gives its
    // version and not an error of its form.
    match value.get("schema").and_then(serde_json::Value::as_u64) {
        Some(v) if v == u64::from(SCHEMA) => {}
        other => return Err(ReadError::Version(other)),
    }
    let Document { schema: _, findings, selection } =
        serde_json::from_value(value).map_err(|e| ReadError::Form(e.to_string()))?;
    Ok((findings_of(findings)?, selection_of(selection)?))
}

fn wire_row(name: &str, outcome: &Outcome) -> WireRow {
    let Outcome { verdict, errno, reason } = outcome;
    WireRow {
        name: name.to_string(),
        verdict: verdict.word().to_string(),
        errno: errno.map(|e| e.0),
        reason: reason.clone(),
    }
}

fn wire_identity(identity: &Identity) -> WireIdentity {
    let Identity {
        uid,
        gid,
        groups,
        uid_map,
        gid_map,
        setgroups,
        cap_eff,
        cap_prm,
        cap_bnd,
        seccomp,
        seccomp_filters,
        no_new_privs,
        threads,
        unreadable,
    } = identity;
    WireIdentity {
        uid: *uid,
        gid: *gid,
        groups: groups.clone(),
        uid_map: uid_map.clone(),
        gid_map: gid_map.clone(),
        setgroups: setgroups.clone(),
        cap_eff: cap_eff.clone(),
        cap_prm: cap_prm.clone(),
        cap_bnd: cap_bnd.clone(),
        seccomp: seccomp.clone(),
        seccomp_filters: seccomp_filters.clone(),
        no_new_privs: no_new_privs.clone(),
        threads: threads.clone(),
        unreadable: unreadable.clone(),
    }
}

fn wire_write(probe: &WriteProbe) -> WireWrite {
    let WriteProbe { path, source, outcome, space, space_error } = probe;
    let Outcome { verdict, errno, reason } = outcome;
    WireWrite {
        path: path.clone(),
        source: source.clone(),
        verdict: verdict.word().to_string(),
        errno: errno.map(|e| e.0),
        reason: reason.clone(),
        space: space.as_ref().map(|Space { blocks_free, block_size, inodes_free }| WireSpace {
            blocks_free: *blocks_free,
            block_size: *block_size,
            inodes_free: *inodes_free,
        }),
        space_errno: space_error.map(|e| e.0),
    }
}

fn findings_of(wire: WireFindings) -> Result<Findings, ReadError> {
    let WireFindings { rows: wire_rows, identity, writable, self_exe } = wire;
    let mut rows: Vec<(&'static str, Outcome)> = Vec::with_capacity(wire_rows.len());
    for WireRow { name, verdict, errno, reason } in wire_rows {
        // The name of the row is the `&'static str` of its probe.
        let probe = probes::find(&name).ok_or_else(|| ReadError::UnknownRow(name.clone()))?;
        if rows.iter().any(|(n, _)| *n == probe.name) {
            return Err(ReadError::DuplicateRow(name));
        }
        rows.push((probe.name, outcome_of(&verdict, errno, reason, &name)?));
    }
    let writable = writable
        .into_iter()
        .map(|WireWrite { path, source, verdict, errno, reason, space, space_errno }| {
            Ok(WriteProbe {
                outcome: outcome_of(&verdict, errno, reason, &path)?,
                path,
                source,
                space: space.map(|WireSpace { blocks_free, block_size, inodes_free }| Space {
                    blocks_free,
                    block_size,
                    inodes_free,
                }),
                space_error: space_errno.map(Errno),
            })
        })
        .collect::<Result<Vec<_>, ReadError>>()?;
    Ok(Findings { rows, identity: identity_of(identity), writable, self_exe })
}

fn identity_of(wire: WireIdentity) -> Identity {
    let WireIdentity {
        uid,
        gid,
        groups,
        uid_map,
        gid_map,
        setgroups,
        cap_eff,
        cap_prm,
        cap_bnd,
        seccomp,
        seccomp_filters,
        no_new_privs,
        threads,
        unreadable,
    } = wire;
    Identity {
        uid,
        gid,
        groups,
        uid_map,
        gid_map,
        setgroups,
        cap_eff,
        cap_prm,
        cap_bnd,
        seccomp,
        seccomp_filters,
        no_new_privs,
        threads,
        unreadable,
    }
}

fn selection_of(wire: WireSelection) -> Result<Selection, ReadError> {
    let WireSelection { rung, rejected, controls_answered, control_notes } = wire;
    let rejected = rejected
        .into_iter()
        .map(|WireRejection { rung, evidence }| {
            let rung = rung_of(&rung)?;
            Ok(Rejection { rung, requirement: rung.requirement(), evidence })
        })
        .collect::<Result<Vec<_>, ReadError>>()?;
    Ok(Selection { rung: rung_of(&rung)?, rejected, controls_answered, control_notes })
}

fn outcome_of(verdict: &str, errno: Option<i32>, reason: String, at: &str) -> Result<Outcome, ReadError> {
    let verdict = VERDICTS
        .into_iter()
        .find(|v| v.word() == verdict)
        .ok_or_else(|| ReadError::Word { field: "verdict", word: verdict.to_string() })?;
    if verdict == Verdict::Denied && errno.is_none() {
        return Err(ReadError::DenialWithoutErrno(at.to_string()));
    }
    Ok(Outcome { verdict, errno: errno.map(Errno), reason })
}

fn rung_of(word: &str) -> Result<Rung, ReadError> {
    RUNGS
        .into_iter()
        .find(|r| r.word() == word)
        .ok_or_else(|| ReadError::Word { field: "rung", word: word.to_string() })
}

#[cfg(any(test, feature = "schema"))]
mod schema;
#[cfg(feature = "schema")]
pub use schema::{SCHEMA_ID, schema};

#[cfg(test)]
mod tests;
