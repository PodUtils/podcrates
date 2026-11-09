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
use crate::kept::Conn;
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
    if let Some(kept) = way.kept
        && let Some(conn) = kept.take(host, resolver.port)
    {
        match send(way, conn, resolver, host, target, accept).await {
            (Ok(response), back) => {
                if let Some(conn) = back {
                    kept.put(host, resolver.port, conn);
                }
                return Ok(response);
            }
            // The server may have closed it: one reconnect.
            (Err(missed), _) => tracing::debug!(%host, why = %missed, "a kept connection failed; a new one follows"),
        }
    }
    let conn = open(way, resolver, host, connect).await?;
    let (answer, back) = send(way, conn, resolver, host, target, accept).await;
    if let (Some(kept), Some(conn)) = (way.kept, back) {
        kept.put(host, resolver.port, conn);
    }
    answer
}

/// A new connection to `host`: the dial within `connect`, then TLS (HTTP/2
/// alone where the resolver serves no HTTP/1.1).
async fn open(way: &Way<'_>, resolver: &Doh, host: &str, connect: Duration) -> Result<Conn, Missed> {
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
        return crate::h2::open(&config, io, host, words).await.map(Conn::Two);
    }
    let tls = tokio_rustls::TlsConnector::from(config).connect(server, io).await.map_err(|e| Missed::tls(&e, words))?;
    Ok(Conn::One(Box::new(tls)))
}

/// One GET on `conn`, and the connection when it can carry the next one. A
/// way with no [`crate::Kept`] asks the server to close it.
async fn send(
    way: &Way<'_>,
    conn: Conn,
    resolver: &Doh,
    host: &str,
    target: &str,
    accept: &str,
) -> (Result<Response, Missed>, Option<Conn>) {
    match conn {
        Conn::Two(mut conn) => match crate::h2::request(&mut conn, host, target, accept, MAX_ANSWER).await {
            Ok(response) => (Ok(response), way.kept.map(|_| Conn::Two(conn))),
            Err(why) => (Err(Missed::words(why)), None),
        },
        Conn::One(tls) => {
            let mut tls = tls;
            let host_header = if resolver.port == 443 {
                if host.contains(':') { format!("[{host}]") } else { host.to_string() }
            } else {
                authority(host, resolver.port)
            };
            let headers = [("Accept", accept)];
            let user_agent = &way.settings.user_agent;
            if way.kept.is_none() {
                let answer =
                    http::exchange(&mut tls, user_agent, "GET", &host_header, target, &headers, b"", MAX_ANSWER).await;
                return (answer.map_err(|e| Missed::words(e.to_string())), None);
            }
            match http::exchange_keep(*tls, user_agent, "GET", &host_header, target, &headers, b"", MAX_ANSWER).await {
                Ok((response, back)) => (Ok(response), back.map(|tls| Conn::One(Box::new(tls)))),
                Err(e) => (Err(Missed::words(e.to_string())), None),
            }
        }
    }
}
