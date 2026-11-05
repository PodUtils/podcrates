//! The hellos of one connection with ECH: the name's list, a sealed hello
//! with it, one more with the server's retry configs where it sent some,
//! and a plain hello after any other failure, each on a TCP connection of
//! its own, one after another. A sealed hello's failure is read by rustls's
//! own error, never by its words.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Dial, Failure, Stage, Target};
use rustls::client::EchStatus;
use rustls::internal::msgs::codec::Codec as _;
use rustls::{ClientConfig, PeerIncompatible};
use tokio::time::Instant;

use super::cache::{self, Cache, Lookup, Why};
use super::{Mode, Outcome, Records, TlsStream, config};
use crate::context::Context;
use crate::hint::{Words, handshake_failure};
use crate::trust::Trust;

/// The bound of a sealed hello's answer: a hop that drops sealed hellos
/// costs this, not the step's whole bound, before the plain hello.
pub const SEALED_WAIT: Duration = Duration::from_secs(10);

/// The bound of the read of a name's record, before the connection.
pub const READ_WITHIN: Duration = Duration::from_secs(4);

/// Where a connection goes, and how.
pub struct Route<'a> {
    /// Where TCP goes, through `dial`.
    pub target: &'a Target,
    /// The name that TLS checks, and that ECH seals.
    pub server_name: &'a str,
    pub trust: &'a Trust,
    /// The way out of each TCP connection: direct, a proxy, a relay.
    pub dial: &'a dyn Dial,
    /// The read of the name's HTTPS record, through the same way out.
    pub records: &'a dyn Records,
    /// The bound of each step: the dial, and each handshake.
    pub limit: Duration,
    /// The protocols offered by ALPN; none for a WebSocket upgrade.
    pub alpn: &'a [&'a [u8]],
}

/// What follows a sealed hello that failed after its connection opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Next {
    /// The server refused it and sent these retry configs, as a list.
    Retry(Vec<u8>),
    /// It failed, and why: a plain hello comes next.
    Refused(String),
}

/// How one sealed hello ended, when it did not open.
enum Failed {
    /// No connection: the plain hello would meet the same.
    Dial(Failure),
    Next(Next),
}

/// The connection of `route`, sealed with the list that `cache` holds for
/// its name or reads for it; plain where there is none, or after a refusal,
/// but in `strict`.
pub(super) async fn sealed(
    context: &Context,
    cache: &Cache,
    mode: Mode,
    route: &Route<'_>,
) -> Result<(TlsStream, Option<SocketAddr>), Failure> {
    let (name, records) = (route.server_name, route.records);
    let within = route.limit.min(READ_WITHIN);
    let read = move || records.ech_list(name, Instant::now() + within);
    let mut list = match cache.lookup(name, read).await {
        Lookup::List(list) => list,
        Lookup::None(Why::NotPublished) => return unsealed(context, mode, route, Outcome::NotPublished).await,
        Lookup::None(Why::ReadFailed(why)) => return unsealed(context, mode, route, Outcome::ReadFailed(why)).await,
        Lookup::None(Why::Cooled) => return unsealed(context, mode, route, Outcome::Cooled).await,
    };
    let mut retried = false;
    loop {
        match attempt(context, &list, route).await {
            Ok(opened) => {
                context.ech.note(name, Outcome::Sealed { retried });
                return Ok(opened);
            }
            Err(Failed::Dial(e)) => return Err(e),
            // One retry, with what the server sent: they replace the list.
            Err(Failed::Next(Next::Retry(configs))) if !retried => {
                cache.remember(name, configs.clone(), cache::NO_TTL);
                (list, retried) = (configs, true);
            }
            Err(Failed::Next(Next::Retry(_))) => {
                cache.refused(name);
                let why = "the server refused the retry configs that it sent".to_string();
                return unsealed(context, mode, route, Outcome::Refused(why)).await;
            }
            Err(Failed::Next(Next::Refused(why))) => {
                cache.refused(name);
                return unsealed(context, mode, route, Outcome::Refused(why)).await;
            }
        }
    }
}

/// A plain hello after `outcome`, which is noted; none in `strict`.
async fn unsealed(
    context: &Context,
    mode: Mode,
    route: &Route<'_>,
    outcome: Outcome,
) -> Result<(TlsStream, Option<SocketAddr>), Failure> {
    if mode == Mode::Strict {
        let why = format!("{}: {}", route.server_name, outcome.reason());
        context.ech.note(route.server_name, Outcome::Strict(why.clone()));
        let words = Outcome::Strict(why).words(&context.words().ech_strict);
        return Err(Failure::new(Stage::Tls, Class::Unavailable, words));
    }
    context.ech.note(route.server_name, outcome);
    plain(context, route).await
}

/// One sealed hello with `list`, on a connection of its own.
async fn attempt(context: &Context, list: &[u8], route: &Route<'_>) -> Result<(TlsStream, Option<SocketAddr>), Failed> {
    let config = config::sealed(context, route.trust, route.server_name, list, route.alpn)
        .map_err(|why| Failed::Next(Next::Refused(why)))?;
    let name = crate::wrap::server_name(route.server_name).map_err(Failed::Dial)?;
    let tcp = route.dial.dial(route.target, Instant::now() + route.limit).await.map_err(Failed::Dial)?;
    let peer = tcp.peer;
    let wait = route.limit.min(SEALED_WAIT);
    let tls = match tokio::time::timeout(wait, tokio_rustls::TlsConnector::from(config).connect(name, tcp.io)).await {
        Err(_) => {
            let why = format!("no answer to the sealed hello within {} s", wait.as_secs());
            return Err(Failed::Next(Next::Refused(why)));
        }
        Ok(Err(e)) => return Err(Failed::Next(after(&e, context.words()))),
        Ok(Ok(tls)) => tls,
    };
    // rustls ends a handshake whose sealed hello the server did not open;
    // one that ended well was opened, which this reads again.
    if tls.get_ref().1.ech_status() != EchStatus::Accepted {
        return Err(Failed::Next(Next::Refused("the server did not confirm the sealed hello".into())));
    }
    Ok((tls, peer))
}

/// What follows a sealed hello that `error` ended: the retry configs of a
/// server that refused it with some, as an ECHConfigList; else why, in the
/// words of [`handshake_failure`].
pub(super) fn after(error: &std::io::Error, words: &Words) -> Next {
    let rejected = match error.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()) {
        Some(rustls::Error::PeerIncompatible(PeerIncompatible::ServerRejectedEncryptedClientHello(configs))) => configs,
        _ => return Next::Refused(handshake_failure(error, words)),
    };
    match rejected {
        // rustls gives the configs as its own type, and names no public way
        // to write them: `Codec` of `rustls::internal` writes the list.
        Some(configs) if !configs.is_empty() => Next::Retry(configs.get_encoding()),
        _ => Next::Refused("the server refused the sealed hello and sent no retry configs".into()),
    }
}

/// A plain hello, as each road makes it: the trust's own configuration, and
/// with ALPN a copy of it that offers `route.alpn`.
pub(super) async fn plain(context: &Context, route: &Route<'_>) -> Result<(TlsStream, Option<SocketAddr>), Failure> {
    let tcp = route.dial.dial(route.target, Instant::now() + route.limit).await?;
    let peer = tcp.peer;
    if route.alpn.is_empty() {
        let tls = context.tls_over(tcp.io, route.server_name, route.trust, route.limit).await?;
        return Ok((tls, peer));
    }
    let kept = context.config_for(route.trust)?;
    let mut config: ClientConfig = (*kept).clone();
    config.alpn_protocols = route.alpn.iter().map(|p| p.to_vec()).collect();
    let tls = crate::tls_over(tcp.io, route.server_name, Arc::new(config), route.limit, context.words()).await?;
    Ok((tls, peer))
}

#[cfg(test)]
#[path = "road_tests.rs"]
mod tests;
