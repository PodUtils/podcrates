//! What a crate tells its consumer, with no output of its own: an event is a
//! line for the user, which the consumer shows in its own words and place;
//! an observer counts the decisions that the crate takes (podnet keeps them
//! as `podnet_decisions_total`). Diagnostics go to `tracing`.

use std::fmt;
use std::sync::Arc;

/// How much a user needs an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// A step that went as planned (a session moved to a new link).
    Info,
    /// A thing that the user should know (a relay host is down, a token
    /// expires soon).
    Warn,
}

/// One event: a kind for a program, and a text for a person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub level: Level,
    /// A stable name of the kind of event, as `relay.failover`.
    pub kind: &'static str,
    /// One line for a person, made safe to print (`crate::text::one_line`).
    pub text: String,
}

impl Event {
    pub fn info(kind: &'static str, text: impl Into<String>) -> Event {
        Event { level: Level::Info, kind, text: crate::text::one_line(&text.into()) }
    }

    pub fn warn(kind: &'static str, text: impl Into<String>) -> Event {
        Event { level: Level::Warn, kind, text: crate::text::one_line(&text.into()) }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The sink of the events of a crate.
pub trait Events: Send + Sync {
    fn event(&self, event: Event);
}

/// A sink that drops each event.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoEvents;

impl Events for NoEvents {
    fn event(&self, _event: Event) {}
}

impl<F: Fn(Event) + Send + Sync> Events for F {
    fn event(&self, event: Event) {
        self(event)
    }
}

/// The counter of the decisions of a crate.
pub trait Observer: Send + Sync {
    /// One more `outcome` of `decision` (`"resolve.source"`, `"doh"`).
    fn decision(&self, decision: &'static str, outcome: &str);
}

/// An observer that counts nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoObserver;

impl Observer for NoObserver {
    fn decision(&self, _decision: &'static str, _outcome: &str) {}
}

/// The events and the observer that a consumer gives a crate.
#[derive(Clone)]
pub struct Report {
    pub events: Arc<dyn Events>,
    pub observer: Arc<dyn Observer>,
}

impl Report {
    pub fn event(&self, event: Event) {
        self.events.event(event)
    }

    pub fn decision(&self, decision: &'static str, outcome: &str) {
        self.observer.decision(decision, outcome)
    }
}

impl Default for Report {
    fn default() -> Report {
        Report { events: Arc::new(NoEvents), observer: Arc::new(NoObserver) }
    }
}

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Report")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn an_event_is_one_safe_line() {
        let e = Event::warn("relay.failover", "host\u{1b}[2J one\nis down");
        assert_eq!(e.text, "host[2J one is down");
        assert_eq!(e.level, Level::Warn);
    }

    #[test]
    fn a_closure_is_a_sink_and_a_report_reaches_it() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let keep = seen.clone();
        let report =
            Report { events: Arc::new(move |e: Event| keep.lock().unwrap().push(e.kind)), ..Report::default() };
        report.event(Event::info("session.moved", "moved"));
        report.decision("resolve.source", "doh");
        assert_eq!(*seen.lock().unwrap(), ["session.moved"]);
    }
}
