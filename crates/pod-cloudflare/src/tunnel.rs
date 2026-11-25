//! The tunnel engine: credentials, edge registration, HA reactors, reconnect
//! and dispatch to the service runtime.

mod handle;
mod plan;
mod reactor;
pub use handle::*;
pub use plan::*;
use reactor::*;

use std::sync::Arc;
use std::time::Duration;

use crate::cloudflare as edge;
use crate::cloudflare::{IpVersionFilter, quic as quic_dial, rpc};
use crate::fail::{Result, bail};
use pod_core::Class;
use pod_core::{Event, Stage};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::info;
use uuid::Uuid;

use crate::Host;
use crate::config::{Config, Credentials, Protocol, ReconnectPolicy, Transport, parse_credentials_file, parse_token};
use crate::ingress::{Ingress, Service};
use crate::metrics::Metrics;
use crate::proxy;
use crate::proxy::Runtime;
use crate::quicktunnel::validate_quick_hostname;

const REGISTER_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

mod verify;
mod warp_targets;
pub use verify::Verified;
use verify::{VERIFY_BUDGET, VERIFY_INTERVAL, verify_fetch};
use warp_targets::{EndpointSource, WarpLink, warp_targets};

/// The builder of a [`Tunnel`] over the [`Host`] of the consumer.
pub struct TunnelBuilder {
    config: Config,
    host: Arc<Host>,
    metrics: Option<Metrics>,
}

impl TunnelBuilder {
    pub fn quick(host: Arc<Host>) -> Self {
        Self { config: Config::default(), host, metrics: None }
    }

    pub fn token(host: Arc<Host>, token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            config: Config { credentials: parse_token(&token.into())?, ..Config::default() },
            host,
            metrics: None,
        })
    }

    pub fn credentials_file(host: Arc<Host>, path: impl Into<std::path::PathBuf>) -> Result<Self> {
        Ok(Self {
            config: Config { credentials: parse_credentials_file(&path.into())?, ..Config::default() },
            host,
            metrics: None,
        })
    }

    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.config.credentials = credentials;
        self
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn service(mut self, service: Service) -> Self {
        self.config.ingress = Ingress::default().catch_all(service);
        self
    }

    pub fn ingress(mut self, ingress: Ingress) -> Self {
        self.config.ingress = ingress;
        self
    }

    pub fn ha_connections(mut self, count: u8) -> Self {
        self.config.ha_connections = count.clamp(1, 8);
        self
    }

    pub fn protocol(mut self, protocol: Protocol) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// How the edge connection is reached: direct on 7844, or carried by WARP
    /// over TCP 443. See [`Transport`].
    pub fn transport(mut self, transport: Transport) -> Self {
        self.config.transport = transport;
        self
    }

    pub fn reconnect(mut self, policy: ReconnectPolicy) -> Self {
        self.config.reconnect = policy;
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = user_agent.into();
        self
    }

    pub fn service_url(mut self, url: impl Into<String>) -> Self {
        self.config.service_url = url.into();
        self
    }

    pub fn verify(mut self, yes: bool) -> Self {
        self.config.verify = yes;
        self
    }

    /// The counters of the tunnel. The consumer gives them when its own
    /// services count into the same counters, or when it adds its own text
    /// ([`Metrics::with_extra`]). With none, the tunnel makes counters with
    /// the name of the consumer.
    pub fn metrics(mut self, metrics: Metrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn build(self) -> Result<Tunnel> {
        if self.config.ingress.is_empty() {
            return Err(crate::fail::new(Stage::Host, Class::Config, NO_INGRESS));
        }
        Ok(Tunnel { config: self.config, host: self.host, metrics: self.metrics })
    }
}

/// The error of a tunnel with no ingress rules.
pub const NO_INGRESS: &str =
    "no ingress rules configured (give ingress:, url: or hello-world: in the file, or an origin flag)";

pub struct Tunnel {
    config: Config,
    host: Arc<Host>,
    metrics: Option<Metrics>,
}

impl Tunnel {
    pub fn builder(host: Arc<Host>) -> TunnelBuilder {
        TunnelBuilder::quick(host)
    }

    pub fn quick(host: Arc<Host>) -> TunnelBuilder {
        TunnelBuilder::quick(host)
    }

    pub fn token(host: Arc<Host>, token: impl Into<String>) -> Result<TunnelBuilder> {
        TunnelBuilder::token(host, token)
    }

    /// Get the credentials (provision or load), register with the edge, and
    /// start to serve.
    pub async fn start(self) -> Result<TunnelHandle> {
        let host = self.host;
        let Config {
            service_url,
            user_agent,
            credentials,
            protocol,
            transport,
            ha_connections,
            reconnect,
            ingress,
            verify,
            failover,
            secrets: _,
        } = self.config;

        // Build the services before the quick tunnel is provisioned: a bad
        // --dir, a --spa with no index.html or a missing --file stops here.
        let metrics = self.metrics.unwrap_or_else(|| Metrics::with_prefix(&host.settings.words.name));
        let runtime = Arc::new(Runtime::build(host.clone(), &ingress, metrics.clone())?);
        let forward_path = crate::ws::forward_path_with(&ingress, &host.settings.forward_path);

        // The UDP probe runs while the quick tunnel is provisioned.
        let udp = host.settings.udp;
        let udp_probe = (udp == crate::udp::Choice::Probe
            && protocol == Protocol::Auto
            && leg_plan(protocol, transport, &host.settings.words).is_ok_and(|p| p.contains(&LegKind::QuicDirect)))
        .then(|| {
            let target = probe_target(&host.settings);
            tokio::task::spawn_blocking(move || crate::udp::probe(target, crate::udp::START_DEADLINE))
        });

        let (auth, tunnel_id, public_url) = resolve_credentials(&host, &credentials, &service_url, &user_agent).await?;
        let account_tag = auth.account_tag.clone();
        let ingress = Arc::new(ingress);

        // The edge legs to try, in order. QUIC on UDP 7844 is the fastest.
        // HTTP/2 on TCP 7844 works where UDP is blocked. WARP carries QUIC
        // inside a MASQUE tunnel on TCP 443, for hosts that reach only 443.
        //
        // Do not probe TCP 7844 to choose: a firewall can close TCP 7844 and
        // keep UDP 7844 open. Each attempt is a real registration. With the
        // staggered failover (the default), the next leg starts when the one
        // before has not registered in 2 s, or at once when it fails; the
        // first leg that registers stays.
        let plan = leg_plan(protocol, transport, &host.settings.words)?;
        let proxy = host.proxy_for(edge_region_name(), crate::services::edge_port());
        let (plan, note) = plan_with_proxy(plan, protocol, proxy.as_deref(), &host.settings.words);
        if let Some(note) = note {
            if matches!(protocol, Protocol::Auto) {
                host.event(Event::info("tunnel.plan", note));
            } else {
                host.event(Event::warn("tunnel.plan", note));
            }
        }
        let egress = match udp_probe {
            Some(task) => task.await.ok(),
            None => None,
        };
        let (plan, note) = plan_with_udp(plan, udp, egress.as_ref(), &host.settings.words)?;
        if let Some(note) = note {
            host.event(Event::info("tunnel.plan", note));
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (legs, stopped) = Legs::new();
        let mut tasks = Vec::new();
        type Kept = Option<Arc<WarpLink>>;
        let limited = plan.len() > 1 && failover == crate::race::Failover::Serial;
        let limit = |kind: LegKind| attempt_limit(kind).filter(|_| limited);
        let mut attempts: Vec<futures::future::BoxFuture<'_, Result<(First, Kept)>>> = Vec::new();
        for kind in &plan {
            let auth = &auth;
            let runtime = runtime.clone();
            let ingress = ingress.clone();
            let host = host.clone();
            let attempt: futures::future::BoxFuture<'_, Result<(First, Kept)>> = match kind {
                LegKind::QuicDirect => Box::pin(bounded(limit(*kind), async move {
                    let edges = direct_targets(&host).await?;
                    let endpoint = quic_dial::build_endpoint(&host.tls, &host.settings.words.protocol)?;
                    let version = &host.settings.client_version;
                    let (conn, control, location) = register_retrying_with(&host.report, || {
                        connect_once(&endpoint, auth, tunnel_id, 0, false, &edges, version)
                    })
                    .await?;
                    Ok((First::Quic(edges, endpoint, conn, control, location), None))
                })),
                LegKind::H2Direct | LegKind::H2Relay => Box::pin(bounded(limit(*kind), async move {
                    let relay = relay_for(*kind, &host)?;
                    let edges = direct_targets(&host).await?;
                    let options = rpc::ConnectionOptions::default_for_quick_tunnel(&host.settings.client_version);
                    let leg = register_retrying_with(&host.report, || {
                        let (runtime, ingress) = (runtime.clone(), ingress.clone());
                        crate::edge_h2::connect(&edges, auth, tunnel_id, 0, &options, runtime, ingress, relay.as_ref())
                    })
                    .await?;
                    Ok((First::H2(edges, leg, relay), None))
                })),
                LegKind::QuicWarp => Box::pin(async move {
                    let (edges, link, endpoint) = warp_targets(&host).await?;
                    let version = &host.settings.client_version;
                    let (conn, control, location) = register_retrying_with(&host.report, || {
                        connect_once(&endpoint, auth, tunnel_id, 0, false, &edges, version)
                    })
                    .await?;
                    let kept = Some(link);
                    Ok((First::Quic(edges, endpoint, conn, control, location), kept))
                }),
            };
            attempts.push(attempt);
        }
        // A loser that also registered: unregister it, then close it.
        let close = |index: usize, (first, _kept): (First, Kept)| -> futures::future::BoxFuture<'static, ()> {
            Box::pin(async move {
                info!(attempt = index, "closing a second registered leg");
                match first {
                    First::Quic(_, _, conn, control, _) => {
                        control.shutdown_graceful(Duration::from_secs(2)).await;
                        conn.close(0u32.into(), b"another leg won");
                    }
                    First::H2(_, leg, _) => {
                        leg.control.shutdown_graceful(Duration::from_secs(2)).await;
                        leg.serve.abort();
                    }
                }
            })
        };
        let won = match crate::race::race(attempts, failover, close).await {
            Ok(won) => won,
            Err(failed) => return Err(no_leg_worked(&plan, failed)),
        };
        for (i, err) in &won.failures {
            let why = err.chain();
            host.event(Event::warn("tunnel.leg_failed", format!("the edge leg {} failed: {why}", plan[*i])));
        }
        let kind = plan[won.index];
        host.event(Event::info("tunnel.leg", format!("the edge leg {kind} won after {} ms", won.after.as_millis())));
        host.report.decision("leg", &kind.to_string());
        let (first, warp_keepalive) = won.value;
        metrics.connection_opened();

        let location = match first {
            First::Quic(edges, endpoint, conn, control, location) => {
                host.event(Event::info(
                    "tunnel.registered",
                    format!("registered with the edge at {location} ({kind})"),
                ));
                let edges = Arc::new(edges);
                let source = match &warp_keepalive {
                    Some(link) => EndpointSource::Warp(link.clone()),
                    None => EndpointSource::Fixed(endpoint.clone()),
                };
                tasks.push(spawn_leg(
                    &legs,
                    reactor(
                        runtime.clone(),
                        ingress.clone(),
                        auth.clone(),
                        tunnel_id,
                        source.clone(),
                        conn,
                        control,
                        metrics.clone(),
                        shutdown_rx.clone(),
                        reconnect.clone(),
                        0,
                        edges.clone(),
                    ),
                ));
                // The other HA legs register in the background.
                for index in 1..ha_connections {
                    let runtime = runtime.clone();
                    let ingress = ingress.clone();
                    let auth = auth.clone();
                    let source = source.clone();
                    let metrics = metrics.clone();
                    let shutdown_rx = shutdown_rx.clone();
                    let reconnect = reconnect.clone();
                    let edges = edges.clone();
                    tasks.push(spawn_leg(&legs, async move {
                        let endpoint = match source.endpoint(1).await {
                            Ok(endpoint) => endpoint,
                            Err(err) => {
                                let why = err.chain();
                                let text = format!("the edge connection {index} did not register: {why}");
                                runtime.host().event(Event::warn("tunnel.leg_failed", text));
                                return None;
                            }
                        };
                        let version = &runtime.host().settings.client_version;
                        match connect_once(&endpoint, &auth, tunnel_id, index, false, &edges, version).await {
                            Ok((conn, control, location)) => {
                                metrics.connection_opened();
                                let text = format!("the edge connection {index} registered at {location}");
                                runtime.host().event(Event::info("tunnel.registered", text));
                                reactor(
                                    runtime,
                                    ingress,
                                    auth,
                                    tunnel_id,
                                    source,
                                    conn,
                                    control,
                                    metrics,
                                    shutdown_rx,
                                    reconnect,
                                    index,
                                    edges,
                                )
                                .await
                            }
                            Err(err) => {
                                let why = err.chain();
                                let text = format!("the edge connection {index} did not register: {why}");
                                runtime.host().event(Event::warn("tunnel.leg_failed", text));
                                None
                            }
                        }
                    }));
                }
                location
            }
            First::H2(edges, leg, relay) => {
                let text = format!("registered with the edge at {} ({kind})", leg.location);
                host.event(Event::info("tunnel.registered", text));
                let location = leg.location.clone();
                let edges = Arc::new(edges);
                let mut first_leg = Some(leg);
                for index in 0..ha_connections {
                    let runtime = runtime.clone();
                    let ingress = ingress.clone();
                    let auth = auth.clone();
                    let metrics = metrics.clone();
                    let shutdown_rx = shutdown_rx.clone();
                    let reconnect = reconnect.clone();
                    let edges = edges.clone();
                    let leg = first_leg.take();
                    let relay = relay.clone();
                    tasks.push(spawn_leg(&legs, async move {
                        let leg = match leg {
                            Some(leg) => Some(leg),
                            None => {
                                let options = rpc::ConnectionOptions::default_for_quick_tunnel(
                                    &runtime.host().settings.client_version,
                                );
                                match crate::edge_h2::connect(
                                    &edges,
                                    &auth,
                                    tunnel_id,
                                    index,
                                    &options,
                                    runtime.clone(),
                                    ingress.clone(),
                                    relay.as_ref(),
                                )
                                .await
                                {
                                    Ok(leg) => {
                                        metrics.connection_opened();
                                        let text =
                                            format!("the edge connection {index} registered at {}", leg.location);
                                        runtime.host().event(Event::info("tunnel.registered", text));
                                        Some(leg)
                                    }
                                    Err(err) => {
                                        let why = err.chain();
                                        let text = format!("the edge connection {index} did not register: {why}");
                                        runtime.host().event(Event::warn("tunnel.leg_failed", text));
                                        return None;
                                    }
                                }
                            }
                        };
                        crate::edge_h2::reactor(
                            leg,
                            edges,
                            auth,
                            tunnel_id,
                            index,
                            runtime,
                            ingress,
                            metrics,
                            shutdown_rx,
                            reconnect,
                            SHUTDOWN_GRACE,
                            relay,
                        )
                        .await
                    }));
                }
                location
            }
        };
        let locations = vec![location];

        Ok(TunnelHandle {
            forward_path,
            url: public_url,
            tunnel_id,
            account_tag,
            locations,
            leg: kind.to_string(),
            metrics,
            runtime,
            shutdown: shutdown_tx,
            tasks,
            stopped,
            verify,
            _warp: warp_keepalive,
        })
    }
}

#[cfg(test)]
mod tests;
