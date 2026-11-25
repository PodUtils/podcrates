//! One HTTP/2 leg: the dial of the edge, TLS, the registration, and the
//! reactor that keeps the leg registered.

use super::*;
use crate::host::EdgeRelay;
use pod_core::Event;

/// Dial the first edge address that answers, register, and serve streams.
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    edges: &[EdgeAddr],
    auth: &rpc::TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: &rpc::ConnectionOptions,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    relay: Option<&EdgeRelay>,
) -> Result<H2Leg> {
    if edges.is_empty() {
        bail!(Stage::Connect, Class::Config, "no edge addresses to dial");
    }
    let mut remaining: Vec<SocketAddr> = crate::cloudflare::for_connection(edges, conn_index)
        .iter()
        .take(5)
        .map(|e| SocketAddr::new(e.ip, e.port))
        .collect();
    let mut last = None;
    while !remaining.is_empty() {
        let (addr, io) = match dial(runtime.host(), &remaining, relay).await {
            Ok((i, io)) => (remaining.remove(i), io),
            Err(err) => {
                last = Some(err);
                break;
            }
        };
        match connect_one(addr, io, auth, tunnel_id, conn_index, options, runtime.clone(), ingress.clone(), relay).await
        {
            Ok(leg) => return Ok(leg),
            Err(err) => {
                debug!(%addr, error = %err.chain(), "HTTP/2 edge attempt failed");
                last = Some(err);
            }
        }
    }
    Err(last.unwrap_or_else(|| crate::fail::new(Stage::Connect, Class::Unavailable, "no edge answered")))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn connect_one(
    addr: SocketAddr,
    io: BoxedIo,
    auth: &rpc::TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: &rpc::ConnectionOptions,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    relay: Option<&EdgeRelay>,
) -> Result<H2Leg> {
    // Through the relay, the session has a byte and a time limit: count
    // the bytes under TLS, as the relay does.
    let host = runtime.host().clone();
    let (io, budget): (BoxedIo, _) = match relay {
        Some(relay) => {
            let (budget, spent, bytes) = Budget::relay(&relay.context);
            let io = Metered::new(io, bytes, spent);
            (Box::new(io), Some(budget))
        }
        None => (io, None),
    };
    let config = edge_tls(&host.tls)?;
    let name = rustls_pki_types::ServerName::try_from(h2_sni().to_string()).or_fail(
        Stage::Tls,
        Class::Config,
        "the HTTP/2 SNI is not a valid name",
    )?;
    let tls = tokio::time::timeout(DIAL_TIMEOUT, tokio_rustls::TlsConnector::from(config).connect(name, io))
        .await
        .map_err(|_| crate::fail::new(Stage::Tls, Class::Timeout, format!("TLS with the edge at {addr} timed out")))?
        .map_err(|e| {
            let text = format!("TLS with the edge at {addr}: {}", crate::fail::tls_text(&e));
            pod_core::Failure::caused(crate::fail::tls_stage(&e), Class::of_io(e.kind()), text, e)
        })?;

    register_served(addr, tls, auth, tunnel_id, conn_index, options, runtime, ingress, budget).await
}

/// Serve HTTP/2 on `io` (after TLS) and register the connection. On an
/// error, the tasks of the connection stop and the connection closes.
#[allow(clippy::too_many_arguments)]
pub(super) async fn register_served<T>(
    addr: SocketAddr,
    io: T,
    auth: &rpc::TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: &rpc::ConnectionOptions,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    budget: Option<Budget>,
) -> Result<H2Leg>
where
    T: AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let Served { control: control_rx, serve, draining } =
        serve_connection(io, runtime, ingress, Keepalive::default(), budget)
            .await
            .with_context(|| format!("the edge at {addr}"))?;
    let serve = AbortOnDrop::new(serve);

    let (request, mut respond) = tokio::time::timeout(REGISTER_TIMEOUT, control_rx)
        .await
        .map_err(|_| {
            crate::fail::new(
                Stage::EdgeRegister,
                Class::Timeout,
                format!("the edge at {addr} did not open the control stream"),
            )
        })?
        .map_err(|_| {
            crate::fail::new(
                Stage::EdgeRegister,
                Class::Unavailable,
                format!("the edge at {addr} closed before the control stream"),
            )
        })?;
    let send = respond.send_response(http::Response::new(()), false).or_fail(
        Stage::EdgeRegister,
        Class::Unavailable,
        "answering the control stream",
    )?;
    let reader = body_reader(request.into_body());
    let writer = body_writer(send);
    let (details, control) = tokio::time::timeout(
        REGISTER_TIMEOUT,
        rpc::register_connection_on(reader, writer, auth, tunnel_id, conn_index, options),
    )
    .await
    .map_err(|_| {
        crate::fail::new(Stage::EdgeRegister, Class::Timeout, "register_connection over HTTP/2 timed out")
    })??;
    info!(conn_index, location = %details.location, "registered over HTTP/2");
    Ok(H2Leg {
        location: details.location,
        control,
        serve: serve.into_inner(),
        draining,
        started: std::time::Instant::now(),
    })
}

/// Open TCP to one of `addrs`: through the relay of `host` (IPv4 only),
/// through its proxy one address at a time, or directly, with the attempt
/// delay between the addresses. Gives the index of the address. Only the
/// leg `http2+relay` gives a `relay`.
pub(super) async fn dial(host: &Host, addrs: &[SocketAddr], relay: Option<&EdgeRelay>) -> Result<(usize, BoxedIo)> {
    let deadline = || tokio::time::Instant::now() + DIAL_TIMEOUT;
    if let Some(relay) = relay {
        // The relay has no IPv6 egress: IPv4 addresses only.
        let mut last = None;
        for (i, addr) in addrs.iter().enumerate().filter(|(_, a)| a.is_ipv4()) {
            match relay.dial.dial(&Target::Addr(*addr), deadline()).await {
                Ok(connected) => return Ok((i, connected.io)),
                Err(err) => last = Some(crate::fail::wrap(err, format!("the edge at {addr} through the relay"))),
            }
        }
        return Err(last.unwrap_or_else(|| {
            crate::fail::new(Stage::Relay, Class::Unsupported, "no IPv4 edge address for the relay")
        }));
    }
    let first = addrs[0];
    if host.proxy_for(&first.ip().to_string(), first.port()).is_some() {
        let mut last = None;
        for (i, addr) in addrs.iter().enumerate() {
            match host.dial.dial(&Target::Addr(*addr), deadline()).await {
                Ok(connected) => return Ok((i, connected.io)),
                Err(err) => last = Some(crate::fail::wrap(err, format!("TCP to the edge at {addr}"))),
            }
        }
        return Err(last.expect("addrs is not empty"));
    }
    let connected =
        host.dial.dial_addrs(&first.ip().to_string(), addrs, deadline()).await.context("TCP to the edge")?;
    let index = addrs.iter().position(|a| Some(*a) == connected.peer).unwrap_or(0);
    Ok((index, connected.io))
}

/// Keep one HTTP/2 leg registered: serve until the connection takes no new
/// stream, then connect a replacement at once (the old connection finishes
/// its open streams), and after a failure try again as `policy` says. Gives
/// the reason when the leg stops for a cause other than `shutdown`.
#[allow(clippy::too_many_arguments)]
pub async fn reactor(
    mut leg: Option<H2Leg>,
    edges: Arc<Vec<EdgeAddr>>,
    auth: rpc::TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    metrics: crate::metrics::Metrics,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    policy: crate::config::ReconnectPolicy,
    grace: Duration,
    relay: Option<EdgeRelay>,
) -> Option<String> {
    use crate::config::Next;
    let mut attempt = 0u32;
    let mut delay = Duration::ZERO;
    loop {
        if let Some(current) = leg.take() {
            let H2Leg { control, serve, mut draining, started, .. } = current;
            tokio::select! {
                _ = shutdown.changed() => {
                    control.shutdown_graceful(grace).await;
                    // The connection closes when the aborted task ends:
                    // after the shutdown, no connection of the leg is open.
                    serve.abort();
                    let _ = serve.await;
                    return None;
                }
                reason = &mut draining => {
                    let reason = reason.unwrap_or_else(|_| "the connection task ended".into());
                    let text = format!("the HTTP/2 edge connection {conn_index} ends ({reason}); a replacement connects now");
                    runtime.host().event(Event::info("tunnel.reconnect", text));
                    // The old connection finishes its open streams on its
                    // own (GOAWAY can come while streams are open). The
                    // control stream stays until then: a reset of it can
                    // make the edge close the connection.
                    tokio::spawn(async move {
                        let _ = serve.await;
                        drop(control);
                    });
                }
            }
            if started.elapsed() >= policy.stable {
                attempt = 0;
            }
            delay = Duration::ZERO;
        }
        if *shutdown.borrow() {
            return None;
        }
        if !delay.is_zero() {
            tokio::select! {
                _ = shutdown.changed() => return None,
                _ = tokio::time::sleep(delay) => {}
            }
        }
        attempt += 1;
        let mut options = rpc::ConnectionOptions::default_for_quick_tunnel(&runtime.host().settings.client_version);
        options.replace_existing = true;
        match connect(&edges, &auth, tunnel_id, conn_index, &options, runtime.clone(), ingress.clone(), relay.as_ref())
            .await
        {
            Ok(next) => {
                let text = format!("the HTTP/2 edge connection {conn_index} reconnected at {}", next.location);
                runtime.host().event(Event::info("tunnel.reconnect", text));
                metrics.reconnect();
                metrics.connection_opened();
                leg = Some(next);
            }
            Err(err) => {
                metrics.error();
                match policy.next(attempt, &err) {
                    Next::Retry(wait) => {
                        let why = err.chain();
                        let text = format!(
                            "the reconnect {attempt} of the HTTP/2 edge connection {conn_index} failed: {why}; the next one is in {} s",
                            wait.as_secs()
                        );
                        runtime.host().event(Event::warn("tunnel.reconnect", text));
                        delay = wait;
                    }
                    Next::Stop(reason) => {
                        let text = format!("the HTTP/2 edge connection {conn_index} stops: {reason}");
                        runtime.host().event(Event::warn("tunnel.leg_stopped", text));
                        return Some(reason);
                    }
                }
            }
        }
    }
}
