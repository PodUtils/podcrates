//! The lines of a watch: one for a watch that holds, one for each finding
//! of a watch that does not. A line of a moved fact names the file, the
//! fact, its value, the new value and the source of the value.

use pod_core::text::one_line;
use pod_services::{Service, Source};

use crate::{Finding, Outcome};

/// The result of one watch of one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub service: String,
    pub kind: String,
    pub outcome: Outcome,
    /// The source of each moved fact, in the order of the findings; none
    /// for a finding that is no moved fact of the file.
    sources: Vec<Option<String>>,
    /// The first run with no answer of this watch (call C153): its lines
    /// say so, and the runner does not count it.
    first: bool,
}

impl Report {
    pub fn new(service: &Service, watch: &pod_services::Watch, outcome: Outcome) -> Report {
        let sources = outcome
            .findings
            .iter()
            .map(|f| match f {
                Finding::Moved { fact, .. } => source_of(service, fact),
                Finding::Failed(_) | Finding::NoAnswer(_) => None,
            })
            .collect();
        Report { service: service.id().to_string(), kind: watch.kind().to_string(), outcome, sources, first: false }
    }

    /// True when the watch found nothing.
    pub fn holds(&self) -> bool {
        self.outcome.holds()
    }

    /// Mark the watch as the first one with no answer: each finding is
    /// [`Finding::NoAnswer`], and the last scheduled run had none for it.
    pub fn mark_first(&mut self) {
        self.first = self.outcome.no_answer();
    }

    /// True for the first run with no answer of this watch.
    pub fn is_first(&self) -> bool {
        self.first
    }

    /// The lines of the watch, each on one line of text:
    ///
    /// * `holds SERVICE KIND: what it saw`
    /// * `skipped SERVICE KIND: why`
    /// * `MOVED SERVICE KIND: services/SERVICE.toml FACT is WAS; the service gives NOW (source: ...)`
    /// * `FAILED SERVICE KIND: why`
    /// * `FAILED (first no answer) SERVICE KIND: why`
    pub fn lines(&self) -> Vec<String> {
        let head = format!("{} {}", self.service, self.kind);
        if let (true, Some(why)) = (self.holds(), &self.outcome.skipped) {
            return vec![one_line(&format!("skipped {head}: {why}"))];
        }
        if self.holds() {
            let seen =
                if self.outcome.seen.is_empty() { "as the file says".into() } else { self.outcome.seen.join("; ") };
            return vec![one_line(&format!("holds {head}: {seen}"))];
        }
        let file = format!("services/{}.toml", self.service);
        self.outcome
            .findings
            .iter()
            .zip(&self.sources)
            .map(|(finding, source)| match finding {
                Finding::Moved { fact, was, now } => {
                    let source = source.as_deref().map(|s| format!(" (source: {s})")).unwrap_or_default();
                    one_line(&format!("MOVED {head}: {file} {fact} is {was}; the service gives {now}{source}"))
                }
                Finding::Failed(why) => one_line(&format!("FAILED {head}: {why}")),
                Finding::NoAnswer(why) if self.first => one_line(&format!("FAILED (first no answer) {head}: {why}")),
                Finding::NoAnswer(why) => one_line(&format!("FAILED {head}: {why}")),
            })
            .collect()
    }
}

/// The source of a fact (`"NAME"`, or a bare name) or of an endpoint
/// (`endpoint NAME`) of the service.
fn source_of(service: &Service, fact: &str) -> Option<String> {
    let name = fact.trim_matches('"');
    let source = match name.strip_prefix("endpoint ") {
        Some(endpoint) => service.endpoint(endpoint.trim_matches('"')).map(|e| e.source()),
        None => service.fact(name).map(|f| f.source()),
    };
    source.map(shown)
}

fn shown(source: &Source) -> String {
    format!("{}, read {}", source.origin(), source.read())
}
