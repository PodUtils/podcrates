//! The conformance matrix: one row for each road and profile, the scores of
//! each classifier, and a skipped row that a run did not measure.
//!
//! The table is the result. The harness prints no pass count: a count hides
//! the row that never ran.

use crate::middlebox::Decision;
use serde_json::json;

/// The three deployment profiles of podnet#501.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Profile {
    Direct,
    Proxy,
    Restricted,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Direct, Profile::Proxy, Profile::Restricted];

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Direct => "direct",
            Profile::Proxy => "proxy",
            Profile::Restricted => "restricted",
        }
    }

    pub fn parse(text: &str) -> Option<Profile> {
        match text {
            "direct" => Some(Profile::Direct),
            "proxy" => Some(Profile::Proxy),
            "restricted" => Some(Profile::Restricted),
            _ => None,
        }
    }
}

/// The score of one classifier on one capture.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub classifier: String,
    pub score: f64,
    pub reason: String,
}

impl Score {
    pub fn new(classifier: impl Into<String>, score: f64, reason: impl Into<String>) -> Score {
        Score { classifier: classifier.into(), score: score.clamp(0.0, 1.0), reason: reason.into() }
    }
}

/// What a row measured, or why it did not run.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Scores(Vec<Score>),
    Skipped(String),
}

/// One road and profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub road: String,
    pub profile: Profile,
    pub outcome: Outcome,
    pub date: String,
    pub decisions: Vec<Decision>,
}

impl Row {
    pub fn scored(
        road: impl Into<String>,
        profile: Profile,
        scores: Vec<Score>,
        date: impl Into<String>,
        decisions: Vec<Decision>,
    ) -> Row {
        Row { road: road.into(), profile, outcome: Outcome::Scores(scores), date: date.into(), decisions }
    }

    pub fn skipped(
        road: impl Into<String>,
        profile: Profile,
        reason: impl Into<String>,
        date: impl Into<String>,
    ) -> Row {
        Row {
            road: road.into(),
            profile,
            outcome: Outcome::Skipped(reason.into()),
            date: date.into(),
            decisions: Vec::new(),
        }
    }

    pub fn scores(&self) -> Option<&[Score]> {
        match &self.outcome {
            Outcome::Scores(scores) => Some(scores),
            Outcome::Skipped(_) => None,
        }
    }

    pub fn skip_reason(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Scores(_) => None,
            Outcome::Skipped(reason) => Some(reason),
        }
    }

    /// The score of one classifier, or None when the row did not run or the
    /// classifier is absent.
    pub fn score(&self, classifier: &str) -> Option<f64> {
        self.scores()?.iter().find(|s| s.classifier == classifier).map(|s| s.score)
    }
}

/// The false-positive report of the benign cover-site sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Benign {
    pub samples: usize,
    pub flagged: usize,
    pub threshold: f64,
}

impl Benign {
    pub fn rate(&self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }
        self.flagged as f64 / self.samples as f64
    }
}

/// The table of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub date: String,
    pub rows: Vec<Row>,
    pub benign: Option<Benign>,
}

/// The matrix is the table: the rows are the roads by the profiles.
pub type Matrix = Table;

impl Table {
    pub fn new(date: impl Into<String>) -> Table {
        Table { date: date.into(), rows: Vec::new(), benign: None }
    }

    pub fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    pub fn set_benign(&mut self, benign: Benign) {
        self.benign = Some(benign);
    }

    /// The table, one row for each line. A skipped row stays in its place
    /// with its reason.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("pod-harness: the road by profile table\n");
        out.push_str(&format!("date {}\n", self.date));
        out.push_str(&format!(
            "{:<16} {:<11} {:<10} {:>6} {:<12} {}\n",
            "road", "profile", "classifier", "score", "date", "reason"
        ));
        for row in &self.rows {
            match &row.outcome {
                Outcome::Scores(scores) => {
                    for score in scores {
                        out.push_str(&format!(
                            "{:<16} {:<11} {:<10} {:>6.3} {:<12} {}\n",
                            row.road,
                            row.profile.as_str(),
                            score.classifier,
                            score.score,
                            row.date,
                            score.reason
                        ));
                    }
                }
                Outcome::Skipped(reason) => {
                    out.push_str(&format!(
                        "{:<16} {:<11} {:<10} {:>6} {:<12} {}\n",
                        row.road,
                        row.profile.as_str(),
                        "skip",
                        "-",
                        row.date,
                        reason
                    ));
                }
            }
        }
        if let Some(benign) = &self.benign {
            out.push_str(&format!(
                "benign cover site: {} of {} samples over {:.3}, false-positive rate {:.3}\n",
                benign.flagged,
                benign.samples,
                benign.threshold,
                benign.rate()
            ));
        }
        out
    }

    pub fn json(&self) -> String {
        let rows: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|row| {
                let mut object = json!({
                    "road": row.road,
                    "profile": row.profile.as_str(),
                    "date": row.date,
                });
                match &row.outcome {
                    Outcome::Scores(scores) => {
                        let scores: Vec<serde_json::Value> = scores
                            .iter()
                            .map(|s| json!({"classifier": s.classifier, "score": s.score, "reason": s.reason}))
                            .collect();
                        object["state"] = json!("scored");
                        object["scores"] = json!(scores);
                        object["decisions"] = json!(
                            row.decisions
                                .iter()
                                .map(|d| json!({"packet": d.packet, "fault": d.fault, "detail": d.detail}))
                                .collect::<Vec<_>>()
                        );
                    }
                    Outcome::Skipped(reason) => {
                        object["state"] = json!("skipped");
                        object["reason"] = json!(reason);
                    }
                }
                object
            })
            .collect();
        let benign = match &self.benign {
            Some(b) => json!({"samples": b.samples, "flagged": b.flagged, "threshold": b.threshold, "rate": b.rate()}),
            None => serde_json::Value::Null,
        };
        serde_json::to_string_pretty(&json!({
            "schema": "pod-harness.table/1",
            "date": self.date,
            "rows": rows,
            "benign": benign,
        }))
        .expect("a table is JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skipped_row_is_visible_and_not_omitted() {
        let mut table = Table::new("2026-11-20");
        table.push(Row::scored(
            "unix",
            Profile::Direct,
            vec![Score::new("field", 0.9, "sni tunnel.example")],
            "2026-11-20",
            Vec::new(),
        ));
        table.push(Row::skipped("icmp", Profile::Restricted, "no raw socket in this lab", "2026-11-20"));
        assert_eq!(table.rows.len(), 2);
        let text = table.render();
        assert!(text.contains("icmp"), "{text}");
        assert!(text.contains("skip"), "{text}");
        assert!(text.contains("no raw socket in this lab"), "{text}");
        assert!(text.contains("0.900"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&table.json()).unwrap();
        assert_eq!(json["rows"].as_array().unwrap().len(), 2);
        assert_eq!(json["rows"][1]["state"], "skipped");
        assert_eq!(json["rows"][1]["reason"], "no raw socket in this lab");
        assert_eq!(json["date"], "2026-11-20");
    }

    #[test]
    fn a_score_stays_in_its_range() {
        assert_eq!(Score::new("field", 2.0, "x").score, 1.0);
        assert_eq!(Score::new("field", -1.0, "x").score, 0.0);
    }

    #[test]
    fn the_benign_rate_is_a_share() {
        let benign = Benign { samples: 4, flagged: 1, threshold: 0.5 };
        assert!((benign.rate() - 0.25).abs() < 1e-9);
        assert_eq!(Benign { samples: 0, flagged: 0, threshold: 0.5 }.rate(), 0.0);
    }

    #[test]
    fn the_profiles_are_the_three_of_the_issue() {
        assert_eq!(Profile::ALL.len(), 3);
        for profile in Profile::ALL {
            assert_eq!(Profile::parse(profile.as_str()), Some(profile));
        }
        assert_eq!(Profile::parse("other"), None);
    }
}
