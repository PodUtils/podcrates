//! The rule of call C153 for the scheduled runs of `services.yml`: a watch
//! whose findings are each [`Finding::NoAnswer`] fails the run only when the
//! last scheduled run had no answer of the same watch too. The first one
//! prints `FAILED (first no answer)` and does not fail the run. A host that
//! does not answer the addresses of one cloud can answer the next day from
//! another; a service that left gives no answer on each day, so the second
//! day finds it.
//!
//! The state is the list of the watches with no answer of a run, one on
//! each line as `SERVICE KIND N` (N: the place of the watch in its service,
//! from 1). The workflow keeps it in the cache of its job. With no state
//! (the first run, a cache that expired), each watch with no answer is a
//! first one. A run by hand has no state, and each finding fails it.
//!
//! [`Finding::NoAnswer`]: crate::Finding::NoAnswer

use std::collections::BTreeSet;

use crate::Report;

/// The first line of a list.
const HEAD: &str = "# The watches with no answer of a scheduled run of services.yml (call C153).";

/// The list of the last scheduled run, and the list of this run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    was: BTreeSet<String>,
    now: Vec<String>,
}

impl State {
    /// The state of the last scheduled run from the text of its list. An
    /// empty text is no state. A line that starts with `#` is a comment.
    pub fn parse(text: &str) -> State {
        let was = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect();
        State { was, now: Vec::new() }
    }

    /// The key of the watch at `index` (from 0) of `service`, of `kind`.
    pub fn key(service: &str, kind: &str, index: usize) -> String {
        format!("{service} {kind} {}", index + 1)
    }

    /// Note the report of the watch `key`. A watch with no answer goes on
    /// the list of this run; the report of one that the last run did not
    /// list is the first one ([`Report::mark_first`]).
    pub fn note(&mut self, key: String, report: &mut Report) {
        if !report.outcome.no_answer() {
            return;
        }
        if !self.was.contains(&key) {
            report.mark_first();
        }
        self.now.push(key);
    }

    /// The watches with no answer of this run, in the order of the run.
    pub fn now(&self) -> &[String] {
        &self.now
    }

    /// The text of the list of this run, for the next scheduled run.
    pub fn text(&self) -> String {
        let mut out = format!("{HEAD}\n");
        for key in &self.now {
            out.push_str(key);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Finding, Outcome};

    fn report(findings: Vec<Finding>) -> Report {
        let text = "id = \"a-node\"\nkind = \"test\"\nparty = \"a test\"\nabout = \"a test\"\n\n\
                    [[endpoint]]\nname = \"ssh\"\nscheme = \"ssh\"\nhost = \"192.0.2.1\"\nport = 22\n\
                    source = { measured = \"a test\", read = 2026-11-12 }\n\n\
                    [[watch]]\nkind = \"ssh-banner\"\nendpoint = \"ssh\"\n";
        let mut services = pod_services::Catalog::new();
        services.lay("test.toml", text).expect("a valid test file");
        let service = services.overlay().last().expect("the service").clone();
        let outcome = Outcome { findings, ..Outcome::default() };
        Report::new(&service, &service.watches()[0], outcome)
    }

    #[test]
    fn a_watch_with_no_answer_is_first_unless_the_last_run_listed_it() {
        let key = State::key("a-node", "ssh-banner", 0);
        assert_eq!(key, "a-node ssh-banner 1");
        let mut none = State::parse("");
        let mut first = report(vec![Finding::no_answer("192.0.2.1:22: no answer")]);
        none.note(key.clone(), &mut first);
        assert!(first.is_first());
        assert_eq!(first.lines(), ["FAILED (first no answer) a-node ssh-banner: 192.0.2.1:22: no answer"]);
        assert_eq!(none.text(), format!("{HEAD}\na-node ssh-banner 1\n"));

        let mut last = State::parse(&none.text());
        let mut second = report(vec![Finding::no_answer("192.0.2.1:22: no answer")]);
        last.note(key.clone(), &mut second);
        assert!(!second.is_first());
        assert_eq!(second.lines(), ["FAILED a-node ssh-banner: 192.0.2.1:22: no answer"]);
        assert_eq!(last.now(), std::slice::from_ref(&key));
    }

    #[test]
    fn another_finding_is_never_held_and_never_listed() {
        let key = State::key("a-node", "ssh-banner", 0);
        for findings in [
            vec![Finding::failed("the node closed before a banner")],
            vec![Finding::no_answer("no answer"), Finding::failed("an answer of another kind")],
            vec![Finding::moved("\"banner\"", "a", "b")],
            Vec::new(),
        ] {
            let mut state = State::parse("");
            let mut report = report(findings);
            state.note(key.clone(), &mut report);
            assert!(!report.is_first(), "{:?}", report.outcome);
            assert!(state.now().is_empty(), "{:?}", report.outcome);
        }
    }
}
