//! The value that the caller gives each call that reaches a relay: the way
//! out, the places of the cache, the settings, the sink of the events, and
//! the state that the sessions of one run share (the gates of each origin,
//! the limits of the relay in use, the races of the rank, the pool of HTTP/2
//! connections, what the QUIC road learned of each host, the tokens of the
//! run). The crate keeps no
//! process state: a binary makes one `Context` and clones it, and a test
//! makes its own.

#[cfg(feature = "quic")]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};

use pod_core::{Event, Events, NoEvents};
use pod_proxy::Proxy;
use pod_ws::Way;

use crate::cache::Cache;
use crate::gate::Gates;
use crate::pool::Limits;
use crate::rank_auto::Races;
use crate::settings::Settings;

/// The proxy that the dialer of the way takes for a host, if any.
pub type ProxyOf = Arc<dyn Fn(&str) -> Option<Proxy> + Send + Sync>;

/// The way out, the cache, the settings and the shared state of the relay
/// client. Clones share the state.
#[derive(Clone)]
pub struct Context {
    /// The TLS context, the dialer and the user agent of each connection.
    pub way: Way,
    /// The proxy that the dialer takes for a host: a health check names
    /// it, and the QUIC road goes only where there is none.
    pub proxy_of: ProxyOf,
    /// The resolver under the dialer, where the caller has one of
    /// `pod-dns`: the addresses that carried a session go to its memory,
    /// and a health check names the source of an address.
    pub resolver: Option<Arc<pod_dns::Resolver>>,
    /// The places of the private files: the tokens, the pool, the rank.
    pub cache: Cache,
    /// The values that the consumer read from its environment and its
    /// settings file.
    pub settings: Arc<Settings>,
    /// Where the lines for the user go.
    pub events: Arc<dyn Events>,
    shared: Arc<Shared>,
}

/// The state that the sessions of a run share.
#[derive(Default)]
struct Shared {
    gates: Gates,
    in_use: RwLock<Limits>,
    races: Arc<Races>,
    h2: pod_ws::h2_road::Pool,
    tokens: crate::token::RunTokens,
    #[cfg(feature = "quic")]
    quic: Mutex<std::collections::HashMap<String, (std::time::Instant, crate::quic_road::dial::Learned)>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context").field("way", &self.way).field("cache", &self.cache).finish_non_exhaustive()
    }
}

impl Context {
    /// A context over `way`, with no proxy of its own to name, no resolver,
    /// the places of `cache`, the default settings and no sink of events.
    pub fn new(way: Way, cache: Cache) -> Context {
        Context {
            way,
            proxy_of: Arc::new(|_| None),
            resolver: None,
            cache,
            settings: Arc::new(Settings::default()),
            events: Arc::new(NoEvents),
            shared: Arc::default(),
        }
    }

    /// The same context with `settings`.
    pub fn with_settings(mut self, settings: Settings) -> Context {
        self.settings = Arc::new(settings);
        self
    }

    /// The same context with the lines for the user sent to `events`.
    pub fn with_events(mut self, events: Arc<dyn Events>) -> Context {
        self.events = events;
        self
    }

    /// The same context with the proxy of each host as `proxy_of` gives it.
    pub fn with_proxy_of(mut self, proxy_of: ProxyOf) -> Context {
        self.proxy_of = proxy_of;
        self
    }

    /// The same context with the resolver under the dialer.
    pub fn with_resolver(mut self, resolver: Arc<pod_dns::Resolver>) -> Context {
        self.resolver = Some(resolver);
        self
    }

    /// The same context with `way`, the state kept.
    pub fn with_way(mut self, way: Way) -> Context {
        self.way = way;
        self
    }

    /// Send `event` to the caller's sink.
    pub fn tell(&self, event: Event) {
        self.events.event(event);
    }

    /// The gates of each relay origin, which the sessions of the run share.
    pub fn gates(&self) -> &Gates {
        &self.shared.gates
    }

    /// The pool of HTTP/2 connections of the run, each host's apart.
    pub fn h2_pool(&self) -> &pod_ws::h2_road::Pool {
        &self.shared.h2
    }

    /// Keep `limits` as the run's, for the budgets of its sessions.
    pub fn set_in_use(&self, limits: Limits) {
        *self.shared.in_use.write().unwrap_or_else(|e| e.into_inner()) = limits;
    }

    /// The limits of the relay that the run's sessions use: none until a
    /// session opened.
    pub fn in_use(&self) -> Limits {
        *self.shared.in_use.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The tokens of the run, by the key of their deployment.
    pub(crate) fn run_tokens(&self) -> &crate::token::RunTokens {
        &self.shared.tokens
    }

    /// The races of relay hosts of the run: a probe of the rank waits for
    /// their end.
    pub fn races(&self) -> &Arc<Races> {
        &self.shared.races
    }

    /// What the QUIC road learned of each host's UDP.
    #[cfg(feature = "quic")]
    pub(crate) fn quic_memory(
        &self,
    ) -> &Mutex<std::collections::HashMap<String, (std::time::Instant, crate::quic_road::dial::Learned)>> {
        &self.shared.quic
    }

    /// Whether the dialer takes no proxy for `host`.
    pub fn direct_to(&self, host: &str) -> bool {
        (self.proxy_of)(host).is_none() && !self.way.dial.names_inside()
    }
}
