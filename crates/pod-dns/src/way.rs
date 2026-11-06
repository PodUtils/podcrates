//! The questions of DNS over HTTPS through one way out: the settings of a
//! resolver, the TLS context of the binary and a dial. A [`crate::Resolver`]
//! gives its own ([`crate::Resolver::way`]); a caller that asks through
//! another way (the proxy of one road) makes one.

use std::net::IpAddr;

use pod_core::Dial;
use pod_proxy::Missed;
use pod_tls::Context;
use tokio::time::Instant;

use crate::doh::{self, Doh, Form};
use crate::https::{self, Published};
use crate::json::{self, Srv};
use crate::settings::Settings;
use crate::{ask, lookup, question, wire};

/// A way to the resolvers of DNS over HTTPS.
#[derive(Clone, Copy)]
pub struct Way<'a> {
    /// The resolvers and the rules of the questions.
    pub settings: &'a Settings,
    /// The TLS of the binary: its provider and its words.
    pub context: &'a Context,
    /// The way out of each connection to a resolver. It must not resolve a
    /// name by DNS over HTTPS through itself: give it a resolver with no way
    /// (`Resolver::new`), as the name of a proxy needs the system.
    pub dial: &'a dyn Dial,
}

impl std::fmt::Debug for Way<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Way").field("dial", &self.dial.label()).finish_non_exhaustive()
    }
}

impl Way<'_> {
    /// The addresses of `host` from the resolvers of the settings in turn,
    /// A, then AAAA ([`Settings::both`]); the first resolver that answers
    /// with addresses wins, and NXDOMAIN does what [`Settings::nxdomain`]
    /// says.
    pub async fn lookup(&self, host: &str, deadline: Instant) -> Result<Vec<IpAddr>, String> {
        self.lookup_with(&self.settings.resolvers, host, deadline).await
    }

    /// [`Way::lookup`] over a given list of resolvers.
    pub async fn lookup_with(&self, resolvers: &[Doh], host: &str, deadline: Instant) -> Result<Vec<IpAddr>, String> {
        let order = doh::order(resolvers, self.settings.names, self.dial.names_inside());
        lookup::over(resolvers, order, self.settings.nxdomain, host, deadline, |i, at| {
            let resolver = &resolvers[i];
            async move { ask::ask_kept(self, resolver, &at, host, deadline).await }
        })
        .await
    }

    /// One resolver, straight at its address: the addresses and the form
    /// that gave them, or why none came, with how its connection ended (a
    /// doctor's question).
    pub async fn ask(&self, resolver: &Doh, host: &str, deadline: Instant) -> Result<(Vec<IpAddr>, Form), Missed> {
        ask::ask_kept(self, resolver, &resolver.ip.to_string(), host, deadline).await
    }

    /// The SRV records of `name`, from the resolvers of the settings in turn.
    pub async fn srv(&self, name: &str, deadline: Instant) -> Result<Vec<Srv>, String> {
        self.srv_with(&self.settings.resolvers, name, deadline).await
    }

    /// [`Way::srv`] over a given list of resolvers.
    pub async fn srv_with(&self, resolvers: &[Doh], name: &str, deadline: Instant) -> Result<Vec<Srv>, String> {
        let parse = |form, body: &[u8]| match form {
            Form::Json => json::parse_srv(body),
            Form::Wire => wire::srv(body, name.trim_end_matches('.')),
        };
        question::first(self, resolvers, name, "SRV", None, parse, deadline).await
    }

    /// The ECH list that `name` publishes in its HTTPS record, from the
    /// resolvers of the settings in turn: `None` where the first resolver
    /// that answers gives none.
    pub async fn https(&self, name: &str, deadline: Instant) -> Result<Option<Published>, String> {
        self.https_with(&self.settings.resolvers, name, deadline).await
    }

    /// [`Way::https`] over a given list of resolvers.
    pub async fn https_with(
        &self,
        resolvers: &[Doh],
        name: &str,
        deadline: Instant,
    ) -> Result<Option<Published>, String> {
        let parse = |form, body: &[u8]| match form {
            Form::Json => https::from_json(body),
            Form::Wire => https::from_wire(body, name.trim_end_matches('.')),
        };
        question::first(self, resolvers, name, "HTTPS", None, parse, deadline).await
    }

    /// The JSON answer to the question of `kind` (`A`, `AAAA`, `SRV`, ...)
    /// for `name`, from the resolvers of the settings that serve the JSON
    /// form, in turn, until one answers; an answer that the name does not
    /// exist is an answer (podnet's `doh_query`).
    pub async fn query(&self, name: &str, kind: &str, deadline: Instant) -> Result<json::Answer, String> {
        let parse = |_, body: &[u8]| json::parse(body);
        question::first(self, &self.settings.resolvers, name, kind, Some(Form::Json), parse, deadline)
            .await
            .map_err(|e| format!("DNS over HTTPS {kind} {name}: {e}"))
    }
}

/// The read of a name's ECH list through this way (`pod_tls::ech`).
#[cfg(feature = "ech")]
impl pod_tls::ech::Records for Way<'_> {
    fn ech_list<'a>(
        &'a self,
        name: &'a str,
        deadline: Instant,
    ) -> pod_core::BoxFuture<'a, Result<Option<pod_tls::ech::Published>, String>> {
        Box::pin(async move { self.https(name, deadline).await.map(|found| found.map(Into::into)) })
    }
}
