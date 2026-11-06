//! The resolver of a consumer: its settings, its cache and its way to DNS
//! over HTTPS. It implements [`pod_core::Resolve`], which the dial of
//! `pod-proxy` asks.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use pod_core::{BoxFuture, Class, Dial, Failure, Resolve, SOURCE_WAIT, Stage};
use pod_tls::Context;
use tokio::time::Instant;

use crate::dns_safe;
use crate::memory::{Memory, now_ms, split};
use crate::order::{Found, Known, Source, resolve_in};
use crate::settings::Settings;
use crate::way::Way;

/// The resolver: see the crate.
pub struct Resolver {
    settings: Settings,
    memory: Memory,
    /// The TLS of the binary and the way out to the resolvers of DNS over
    /// HTTPS; none: DNS over HTTPS is not asked.
    doh: Option<(Arc<Context>, Arc<dyn Dial>)>,
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("settings", &self.settings)
            .field("doh", &self.doh.as_ref().map(|(_, dial)| dial.label()))
            .finish_non_exhaustive()
    }
}

impl Resolver {
    /// A resolver of `settings` that asks no DNS over HTTPS: an IP literal,
    /// the pins, the system and the cache, for the dial of DNS over HTTPS
    /// itself.
    pub fn new(settings: Settings) -> Resolver {
        let memory = Memory::new(settings.memory);
        Resolver { settings, memory, doh: None }
    }

    /// The same resolver, which asks DNS over HTTPS through `dial`, with the
    /// TLS of `context`. The dial's own resolver must ask no DNS over HTTPS
    /// through it (a resolver of [`Resolver::new`]): a resolver's name, and
    /// the name of a proxy, then need no answer of the way that they open.
    pub fn with_doh(mut self, context: Arc<Context>, dial: Arc<dyn Dial>) -> Resolver {
        self.doh = Some((context, dial));
        self
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The cache of addresses and the record of the dialer's connections.
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Whether the resolver asks DNS over HTTPS.
    pub fn asks_doh(&self) -> bool {
        self.doh.is_some()
    }

    /// The way of the resolver to DNS over HTTPS, for the other questions
    /// ([`Way::srv`], [`Way::https`], [`Way::query`]); none with no way.
    pub fn way(&self) -> Option<Way<'_>> {
        self.doh.as_ref().map(|(context, dial)| Way { settings: &self.settings, context, dial: &**dial })
    }

    /// The addresses to try for `host:port`, from the first source after
    /// `after` (from the start with none) that gives any: the dialer asks
    /// again when the addresses of a source that [falls back](Source::falls_back)
    /// did not connect. The error names every source that was tried.
    pub async fn find(&self, host: &str, port: u16, deadline: Instant, after: Option<Source>) -> Result<Found, String> {
        let known = self.known(host);
        // DNS over HTTPS leaves the old addresses their turn.
        let doh_by =
            if known.old.is_empty() { deadline } else { deadline.checked_sub(SOURCE_WAIT).unwrap_or(deadline) };
        resolve_in(&known, port, after, self.system(host, port, deadline), self.doh_lookup(host, doh_by)).await
    }

    /// What is known of `host` before the network is asked.
    fn known(&self, host: &str) -> Known {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        let (fresh, old) = split(&self.memory.remembered(host), now_ms());
        Known { literal: bare.parse().ok(), pins: self.settings.pinned(host), fresh, old, asks: dns_safe(host) }
    }

    /// The system resolver, within `deadline`.
    async fn system(&self, host: &str, port: u16, deadline: Instant) -> Result<Vec<SocketAddr>, String> {
        match tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await {
            Ok(Ok(addrs)) => {
                let addrs: Vec<SocketAddr> = addrs.collect();
                if addrs.is_empty() { Err("no addresses".to_string()) } else { Ok(addrs) }
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("timed out".to_string()),
        }
    }

    /// DNS over HTTPS through the way of the resolver, or why it is not
    /// asked.
    async fn doh_lookup(&self, host: &str, deadline: Instant) -> Result<Vec<IpAddr>, String> {
        match self.way() {
            Some(way) => way.lookup(host, deadline).await,
            None => Err("not asked for this name".into()),
        }
    }
}

impl Resolve for Resolver {
    fn label(&self) -> String {
        match self.doh {
            Some(_) => "the resolver of pod-dns".into(),
            None => "the resolver of pod-dns with no DNS over HTTPS".into(),
        }
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        deadline: Instant,
        after: Option<pod_core::Source>,
    ) -> BoxFuture<'a, Result<pod_core::Found, Failure>> {
        Box::pin(async move {
            match self.find(host, port, deadline, after.and_then(Source::of)).await {
                Ok(found) => {
                    tracing::debug!(%host, source = found.source.name(), addrs = ?found.addrs, "resolved");
                    Ok(pod_core::Found { addrs: found.addrs, source: found.source.core(), more: found.more })
                }
                Err(detail) => {
                    let class = if Instant::now() >= deadline { Class::Timeout } else { Class::Unavailable };
                    Err(Failure::new(Stage::Dns, class, detail))
                }
            }
        })
    }

    fn pinned(&self, host: &str) -> Vec<IpAddr> {
        self.settings.pinned(host)
    }

    fn note_dialed(&self, host: &str, peer: SocketAddr, source: pod_core::Source) {
        if let Some(source) = Source::of(source) {
            self.memory.note_dialed(host, peer, source);
        }
    }
}

/// The read of a name's ECH list through the way of the resolver; with
/// none, each read fails, so the name goes plain (and `strict` refuses it).
#[cfg(feature = "ech")]
impl pod_tls::ech::Records for Resolver {
    fn ech_list<'a>(
        &'a self,
        name: &'a str,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Option<pod_tls::ech::Published>, String>> {
        Box::pin(async move {
            match self.way() {
                Some(way) => way.https(name, deadline).await.map(|found| found.map(Into::into)),
                None => Err("the resolver asks no DNS over HTTPS".into()),
            }
        })
    }
}
