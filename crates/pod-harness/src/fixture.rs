//! The fixtures of a run: the models, the cases and the captures. A run
//! turns them into the table and the simulator decisions.

use crate::capture::Capture;
use crate::error::{Error, Result};
use crate::field::FieldMatcher;
use crate::flow::FlowStats;
use crate::matrix::{Benign, Profile, Row, Score, Table};
use crate::middlebox::{Decision, Fault, Middlebox};
use crate::prefix::Prefix;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The threshold of the benign false-positive report.
pub const BENIGN_THRESHOLD: f64 = 0.5;

/// One case of the matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub road: String,
    pub profile: Profile,
    /// The capture, relative to the fixture directory.
    pub capture: Option<String>,
    pub faults: Vec<Fault>,
    /// The reason that the case did not run, when it did not.
    pub skip: Option<String>,
}

/// The models, the cases and the paths of a fixture directory.
#[derive(Clone, Debug)]
pub struct Fixtures {
    pub dir: PathBuf,
    pub field: FieldMatcher,
    pub flow: FlowStats,
    pub prefix: Prefix,
    pub cases: Vec<Case>,
}

/// The result of a run.
#[derive(Clone, Debug)]
pub struct Run {
    pub table: Table,
    /// The decisions of the simulator for each road and profile.
    pub decisions: BTreeMap<String, Vec<Decision>>,
    pub benign: Benign,
    pub elapsed: Duration,
}

impl Fixtures {
    pub fn load(dir: &Path) -> Result<Fixtures> {
        let field = FieldMatcher::load(&dir.join("rules.json"))?;
        let flow = FlowStats::load(&dir.join("flow.json"))?;
        let prefix = Prefix::load(&dir.join("prefix.json"))?;
        let text = std::fs::read_to_string(dir.join("cases.json"))
            .map_err(|e| Error::io("read the cases", &dir.join("cases.json"), e))?;
        let cases = parse_cases(&text)?;
        Ok(Fixtures { dir: dir.to_path_buf(), field, flow, prefix, cases })
    }

    pub fn run(&self, date: &str) -> Result<Run> {
        let started = Instant::now();
        let mut table = Table::new(date);
        let mut decisions = BTreeMap::new();
        for case in &self.cases {
            let key = format!("{} / {}", case.road, case.profile.as_str());
            if let Some(reason) = &case.skip {
                table.push(Row::skipped(&case.road, case.profile, reason, date));
                continue;
            }
            let capture = self.capture(case)?;
            let mut simulator = Middlebox::new(case.faults.clone());
            let processed = simulator.apply(&capture);
            let (field, field_reason) = self.field.score(&processed);
            let (flow, flow_reason) = self.flow.score(&processed);
            let (prefix, prefix_reason) = self.prefix.score(&processed);
            let row = Row::scored(
                &case.road,
                case.profile,
                vec![
                    Score::new("field", field, field_reason),
                    Score::new("flow", flow, flow_reason),
                    Score::new("prefix", prefix, prefix_reason),
                ],
                date,
                simulator.decisions().to_vec(),
            );
            decisions.insert(key, simulator.decisions().to_vec());
            table.push(row);
        }
        let benign = self.benign()?;
        table.set_benign(benign.clone());
        Ok(Run { table, decisions, benign, elapsed: started.elapsed() })
    }

    fn capture(&self, case: &Case) -> Result<Capture> {
        let name = case
            .capture
            .as_deref()
            .ok_or_else(|| Error::new(format!("the case {} has no capture and no skip", case.road)))?;
        if Path::new(name).is_absolute() || name.split('/').any(|part| part == "..") {
            return Err(Error::new(format!("the capture {name:?} is not a plain relative path")));
        }
        Capture::load(&self.dir.join(name))
    }

    /// The benign cover-site sample: a score over the threshold is a false
    /// positive. The rate is printed with the table.
    pub fn benign(&self) -> Result<Benign> {
        let capture = Capture::load(&self.dir.join("captures/benign.cap"))?;
        let (field, _) = self.field.score(&capture);
        let (flow, _) = self.flow.score(&capture);
        let (prefix, _) = self.prefix.score(&capture);
        let flagged = [field, flow, prefix].iter().any(|score| *score >= BENIGN_THRESHOLD);
        Ok(Benign { samples: 1, flagged: usize::from(flagged), threshold: BENIGN_THRESHOLD })
    }
}

fn parse_cases(text: &str) -> Result<Vec<Case>> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let cases = value
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::new("the case file has no cases array"))?;
    let mut out: Vec<Case> = Vec::with_capacity(cases.len());
    for case in cases {
        let road = case
            .get("road")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::new("a case has no road"))?
            .to_string();
        let profile = case
            .get("profile")
            .and_then(serde_json::Value::as_str)
            .and_then(Profile::parse)
            .ok_or_else(|| Error::new(format!("the case {road} has no profile of direct, proxy or restricted")))?;
        let skip = case.get("skip").and_then(serde_json::Value::as_str).map(str::to_string);
        let capture = case.get("capture").and_then(serde_json::Value::as_str).map(str::to_string);
        if skip.is_some() == capture.is_some() {
            return Err(Error::new(format!("the case {road} needs exactly one of capture and skip")));
        }
        let mut faults = Vec::new();
        if let Some(list) = case.get("faults") {
            let list = list.as_array().ok_or_else(|| Error::new(format!("the faults of {road} are not an array")))?;
            for fault in list {
                faults.push(Fault::from_json(fault)?);
            }
        }
        if out.iter().any(|c| c.road == road && c.profile == profile) {
            return Err(Error::new(format!("the case {road} / {} is twice", profile.as_str())));
        }
        out.push(Case { road, profile, capture, faults, skip });
    }
    if out.is_empty() {
        return Err(Error::new("the case file has no case"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_file_needs_one_of_capture_and_skip() {
        assert!(parse_cases(r#"{"cases":[{"road":"x","profile":"direct"}]}"#).is_err());
        assert!(parse_cases(r#"{"cases":[{"road":"x","profile":"direct","skip":"a","capture":"b"}]}"#).is_err());
        assert!(parse_cases(r#"{"cases":[{"road":"x","profile":"other","skip":"a"}]}"#).is_err());
        assert!(parse_cases(r#"{"cases":[]}"#).is_err());
        let cases = parse_cases(r#"{"cases":[{"road":"x","profile":"direct","skip":"a"}]}"#).unwrap();
        assert_eq!(cases.len(), 1);
    }

    #[test]
    fn a_repeated_road_and_profile_is_refused() {
        let text = r#"{"cases":[
            {"road":"x","profile":"direct","skip":"a"},
            {"road":"x","profile":"direct","skip":"b"}
        ]}"#;
        assert!(parse_cases(text).is_err());
    }
}
