//! One GET to a resolver through a way: the dial of the way to the
//! resolver's address or name, TLS verified for the same with the trust of
//! the settings (HTTP/2 alone where the resolver serves no HTTP/1.1), then
//! one request. A failure of the dial or of the handshake keeps its cut, so
//! that a doctor counts a resolver that the egress reset as it counts a
//! relay host, by the kind of the error, never by the system's words; one
//! after the handshake has none.
//!
//! The hello is plain: the read of a name's list for Encrypted Client Hello
//! goes through here, so that it never waits on a read of its own.

use std::time::Duration;

use pod_core::{Class, Target};
use pod_proxy::http::{self, Response};
use pod_proxy::{Cut, Missed, authority};
use tokio::time::Instant;

use crate::MAX_ANSWER;
use crate::doh::Doh;
use crate::way::Way;

/// GET `target` of `resolver` at `host`, its address or its name: the dial
/// within `connect`, then the rest; the whole within `limit` when one is
/// given, else the caller bounds it.
pub(crate) async fn get(
    way: &Way<'_>,
    resolver: &Doh,
    host: &str,
    target: &str,
    accept: &str,
    connect: Duration,
    limit: Option<Duration>,
) -> Result<Response, Missed> {
    let ask = get_now(way, resolver, host, target, accept, connect);
    match limit {
        None => ask.await,
        Some(limit) => tokio::time::timeout(limit, ask).await.unwrap_or_else(|_| {
            Err(Missed { why: format!("no answer within {} s", limit.as_secs()), cut: Some(Cut::TimedOut) })
        }),
    }
}

async fn get_now(
    way: &Way<'_>,
    resolver: &Doh,
    host: &str,
    target: &str,
    accept: &str,
    connect: Duration,
) -> Result<Response, Missed> {
    let config = way.context.config_for(&way.settings.trust).map_err(|e| Missed::words(e.to_string()))?;
    let server = rustls_pki_types::ServerName::try_from(host.to_string()).map_err(|e| Missed::words(e.to_string()))?;
    let to = Target::new(host, resolver.port);
    let dialed = tokio::time::timeout(connect, way.dial.dial(&to, Instant::now() + connect)).await;
    let io = match dialed {
        Err(_) => {
            let why = format!("no connection within {} s", connect.as_secs());
            return Err(Missed { why, cut: Some(Cut::TimedOut) });
        }
        Ok(Err(failure)) if failure.class == Class::Timeout => {
            let why = format!("no connection within {} s", connect.as_secs());
            return Err(Missed { why, cut: Some(Cut::TimedOut) });
        }
        Ok(Err(failure)) => return Err(Missed { why: failure.to_string(), cut: Cut::of_failure(&failure) }),
        Ok(Ok(connected)) => connected.io,
    };
    let words = way.context.words();
    if resolver.http2 {
        return crate::h2::get_over_kept(&config, io, host, target, accept, MAX_ANSWER, words).await;
    }
    let mut tls =
        tokio_rustls::TlsConnector::from(config).connect(server, io).await.map_err(|e| Missed::tls(&e, words))?;
    let host_header = if resolver.port == 443 {
        if host.contains(':') { format!("[{host}]") } else { host.to_string() }
    } else {
        authority(host, resolver.port)
    };
    let headers = [("Accept", accept)];
    let user_agent = &way.settings.user_agent;
    http::exchange(&mut tls, user_agent, "GET", &host_header, target, &headers, b"", MAX_ANSWER)
        .await
        .map_err(|e| Missed::words(e.to_string()))
}
