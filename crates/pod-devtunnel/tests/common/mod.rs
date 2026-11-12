//! The helpers of the tests: the way out of a caller (a direct dial, TLS
//! on ring), the settings of a stand-in, the user's token, the events and
//! the lines of `tracing`, kept.

#![allow(dead_code)]

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{Event, Events, Observer, Report};
use pod_devtunnel::standin::{Plan, Standin};
use pod_devtunnel::{Base, Relay, Scheme, Settings, UserToken, Way};
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, ProxyChoice};
use pod_tls::{Context, Trust, Words};

/// The user's token of the tests: a made-up value.
pub const USER_TOKEN: &str = "made-up-user-token-of-the-tests";

/// The way out of a caller: a direct dial, the TLS of ring.
pub fn way() -> Way {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podssh"))));
    let dialer: Arc<dyn pod_core::Dial> =
        Arc::new(Dialer::new(pod_proxy::Settings::new("podssh", ProxyChoice::Direct), Arc::new(System::new())));
    let http = Client::new(tls.clone(), dialer.clone()).with_user_agent("podssh/0.0.0-test");
    let ws = pod_ws::Way::new(tls, dialer, "podssh/0.0.0-test");
    Way { http, ws, trust: Trust::Default }
}

/// The settings of the stand-in at `base`: short bounds, no wait between
/// the tries of a reconnect.
pub fn settings(base: &str) -> Settings {
    Settings {
        management: Base::parse(base).expect("the base of the stand-in"),
        timeout: Duration::from_secs(10),
        backoff: |_| Duration::ZERO,
        reconnect_tries: 3,
        ..Settings::default()
    }
}

/// The user's token of the tests.
pub fn user() -> UserToken {
    UserToken::new(Scheme::Github, USER_TOKEN).expect("a token")
}

/// The plan of a stand-in that takes [`user`].
pub fn plan() -> Plan {
    Plan::echo("github", USER_TOKEN)
}

/// The events and the decisions of a test, kept.
#[derive(Default)]
pub struct Kept {
    pub events: Mutex<Vec<Event>>,
    pub decisions: Mutex<Vec<(String, String)>>,
}

impl Events for Kept {
    fn event(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

impl Observer for Kept {
    fn decision(&self, decision: &'static str, outcome: &str) {
        self.decisions.lock().unwrap().push((decision.into(), outcome.into()));
    }
}

impl Kept {
    pub fn kinds(&self) -> Vec<&'static str> {
        self.events.lock().unwrap().iter().map(|e| e.kind).collect()
    }

    pub fn texts(&self) -> Vec<String> {
        self.events.lock().unwrap().iter().map(|e| e.to_string()).collect()
    }
}

/// A report whose events and decisions go to the answer.
pub fn report() -> (Report, Arc<Kept>) {
    let kept = Arc::new(Kept::default());
    (Report { events: kept.clone(), observer: kept.clone() }, kept)
}

/// A stand-in of `plan`, and a client connected to its tunnel by name.
pub async fn connected(plan: Plan) -> (Standin, Relay, Arc<Kept>) {
    let standin = Standin::start(plan.clone()).await.expect("the stand-in");
    let (report, kept) = report();
    let locator = pod_devtunnel::Locator::parse(&plan.name).unwrap();
    let relay = Relay::connect(way(), settings(&standin.base()), locator, user(), report)
        .await
        .unwrap_or_else(|e| panic!("connect: {}", e.chain()));
    (standin, relay, kept)
}

/// Each line of `tracing` of this thread, with each field, at each level.
#[derive(Clone, Default)]
pub struct Lines(pub Arc<Mutex<Vec<String>>>);

struct Fields<'a>(&'a mut String);

impl tracing::field::Visit for Fields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl tracing::Subscriber for Lines {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut line = format!("span {} ", span.metadata().name());
        span.record(&mut Fields(&mut line));
        self.0.lock().unwrap().push(line);
        tracing::span::Id::from_u64(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut line = String::from("record ");
        values.record(&mut Fields(&mut line));
        self.0.lock().unwrap().push(line);
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut line = format!("{} ", event.metadata().target());
        event.record(&mut Fields(&mut line));
        self.0.lock().unwrap().push(line);
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}
