//! The plan of the legs: which legs to try, in which order, with which
//! limits; the credentials and the edge addresses that each leg needs.

use super::*;
use crate::Words;
use crate::fail::{Context, OrFail};

pub(super) async fn resolve_credentials(
    host: &Host,
    credentials: &Credentials,
    service_url: &str,
    user_agent: &str,
) -> Result<(rpc::TunnelAuth, Uuid, Option<String>)> {
    match credentials {
        Credentials::Quick => {
            let tunnel = crate::quicktunnel::provision(
                host,
                service_url,
                crate::quicktunnel::AuthMode::Public,
                user_agent,
                Duration::from_secs(30),
                host.settings.provision_budget,
            )
            .await
            .map_err(|e| {
                // A 429 is a temporary refusal (exit 75), with the wait in the
                // refusal. Each other failure makes the service unavailable.
                let class = match crate::quicktunnel::retry_of(&e) {
                    Some(r) if r.status == 429 => Class::TempFail,
                    _ => Class::Unavailable,
                };
                // The failure names the step of the provision already.
                let mut failure = e;
                failure.class = class;
                failure
            })?;
            let secret = tunnel.secret_bytes().context("the quick tunnel secret")?;
            let url = validate_quick_hostname(&tunnel.hostname)?;
            let tunnel_id = Uuid::parse_str(&tunnel.id).or_fail(
                Stage::EdgeRegister,
                Class::Protocol,
                "quick tunnel id is not a UUID",
            )?;
            Ok((rpc::TunnelAuth { account_tag: tunnel.account_tag, tunnel_secret: secret }, tunnel_id, Some(url)))
        }
        Credentials::Token(token) => {
            let (auth, tunnel_id) = split_explicit(parse_token(token)?)?;
            Ok((auth, tunnel_id, None))
        }
        Credentials::File(path) => {
            let (auth, tunnel_id) = split_explicit(parse_credentials_file(path)?)?;
            Ok((auth, tunnel_id, None))
        }
        Credentials::Explicit { account_tag, tunnel_id, secret } => {
            Ok((rpc::TunnelAuth { account_tag: account_tag.clone(), tunnel_secret: secret.clone() }, *tunnel_id, None))
        }
    }
}

pub(super) fn split_explicit(credentials: Credentials) -> Result<(rpc::TunnelAuth, Uuid)> {
    match credentials {
        Credentials::Explicit { account_tag, tunnel_id, secret } => {
            Ok((rpc::TunnelAuth { account_tag, tunnel_secret: secret }, tunnel_id))
        }
        _ => bail!(Stage::Host, Class::Config, "expected explicit credentials"),
    }
}

/// One way to reach the edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegKind {
    /// QUIC on UDP 7844.
    QuicDirect,
    /// HTTP/2 on TCP 7844.
    H2Direct,
    /// QUIC inside a WARP MASQUE tunnel on TCP 443.
    QuicWarp,
    /// HTTP/2 to TCP 7844 in a WebSocket to a relay on TCP 443.
    H2Relay,
}

impl std::fmt::Display for LegKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LegKind::QuicDirect => "quic",
            LegKind::H2Direct => "http2",
            LegKind::QuicWarp => "quic+warp",
            LegKind::H2Relay => "http2+relay",
        })
    }
}

/// The edge legs to try, in order, for a protocol and a transport. An
/// error names the setting of the protocol in the words of the consumer.
pub fn leg_plan(protocol: Protocol, transport: Transport, words: &Words) -> Result<Vec<LegKind>> {
    let flag = &words.protocol;
    use LegKind::*;
    Ok(match (transport, protocol) {
        (Transport::Direct, Protocol::Quic) => vec![QuicDirect],
        (Transport::Direct, Protocol::Http2) => vec![H2Direct],
        (Transport::Direct, Protocol::Auto) => vec![QuicDirect, H2Direct],
        (Transport::Warp, Protocol::Http2) => {
            bail!(Stage::Host, Class::Usage, "the WARP transport carries QUIC only; use {flag} quic or auto")
        }
        (Transport::Warp, _) => vec![QuicWarp],
        (Transport::Auto, Protocol::Quic) => vec![QuicDirect, QuicWarp],
        (Transport::Auto, Protocol::Http2) => vec![H2Direct],
        (Transport::Auto, Protocol::Auto) => vec![QuicDirect, H2Direct, QuicWarp],
        (Transport::Relay, Protocol::Quic) => {
            bail!(Stage::Host, Class::Usage, "the relay carries TCP only; use {flag} http2 or auto")
        }
        (Transport::Relay, _) => vec![H2Relay],
    })
}

pub(crate) use crate::services::edge_region_name;

/// With a proxy, the protocol `auto` does not try QUIC on UDP 7844: no
/// proxy carries UDP, so the packets go around the proxy or are lost. QUIC
/// inside WARP stays, because MASQUE runs on TCP 443 through the proxy.
/// Gives the plan and the line that says why.
pub fn plan_with_proxy(
    plan: Vec<LegKind>,
    protocol: Protocol,
    proxy: Option<&str>,
    words: &Words,
) -> (Vec<LegKind>, Option<String>) {
    let Some(proxy) = proxy else {
        return (plan, None);
    };
    if !plan.contains(&LegKind::QuicDirect) {
        return (plan, None);
    }
    // An explicit protocol quic keeps the leg, with a note that it does not
    // use the proxy.
    if !matches!(protocol, Protocol::Auto) {
        return (
            plan,
            Some(format!(
                "a proxy is set ({proxy}); QUIC uses UDP 7844 and goes around the proxy, so it can fail here ({} http2 goes through the proxy)",
                words.protocol
            )),
        );
    }
    let kept: Vec<LegKind> = plan.into_iter().filter(|k| *k != LegKind::QuicDirect).collect();
    let note =
        format!("a proxy is set ({proxy}); no proxy carries UDP, so {} does not try QUIC on UDP 7844", words.name);
    (kept, Some(note))
}

/// Apply the UDP choice and the result of the UDP probe to the plan. `off`
/// removes QUIC on UDP 7844. With `probe`, a refused probe removes it; an
/// unknown or answered probe keeps the order. The probe runs only for the
/// protocol `auto`, so `egress` is `None` for an explicit protocol.
pub fn plan_with_udp(
    plan: Vec<LegKind>,
    udp: crate::udp::Choice,
    egress: Option<&crate::udp::Egress>,
    words: &Words,
) -> Result<(Vec<LegKind>, Option<String>)> {
    use crate::udp::{Choice, Egress};
    if !plan.contains(&LegKind::QuicDirect) {
        return Ok((plan, None));
    }
    let why = match (udp, egress) {
        (Choice::Off, _) => words.udp_off.clone(),
        (Choice::Probe, Some(Egress::Refused(detail))) => {
            format!("the UDP probe was refused ({detail})")
        }
        (Choice::Probe, Some(other)) => {
            tracing::debug!(result = other.name(), detail = other.detail(), "UDP probe");
            return Ok((plan, None));
        }
        _ => return Ok((plan, None)),
    };
    let kept: Vec<LegKind> = plan.into_iter().filter(|k| *k != LegKind::QuicDirect).collect();
    if kept.is_empty() {
        return Err(crate::fail::new(
            Stage::Host,
            Class::Config,
            format!("{why}, and the only leg is QUIC on UDP 7844: use {} auto or http2", words.protocol),
        ));
    }
    Ok((kept, Some(format!("{why}: {} does not try QUIC on UDP 7844", words.name))))
}

/// The time from the first refusal with retry: yes of a registration in
/// which a new try can start (PodUtils/podnet#685: the edge can answer
/// "Tunnel not found" to a new quick tunnel for more than 2 s).
pub(super) const REGISTER_BUDGET: Duration = Duration::from_secs(30);

/// The shortest and the longest wait between two tries of a registration.
pub(super) const REGISTER_WAIT_MIN: Duration = Duration::from_secs(1);

pub(super) const REGISTER_WAIT_MAX: Duration = Duration::from_secs(5);

/// The budget of a leg whose one try has the budget `one_try`: the first
/// try, the time budget in which more tries start, and the last try.
pub(super) const fn leg_budget(one_try: Duration) -> Duration {
    one_try.saturating_mul(2).saturating_add(REGISTER_BUDGET)
}

/// The wait before retry `retry` (1 is the first): the wait that the edge
/// asks for or the backoff, whichever is longer, from 1 s to 5 s.
pub(super) fn register_wait(retry_after: Duration, retry: u32) -> Duration {
    retry_after.max(pod_core::backoff::backoff(retry)).clamp(REGISTER_WAIT_MIN, REGISTER_WAIT_MAX)
}

/// The budget of one try of the QUIC leg on one edge address: the QUIC
/// handshake and the registration.
pub(super) const QUIC_TRY_BUDGET: Duration = quic_dial::HANDSHAKE_TIMEOUT.saturating_add(REGISTER_TIMEOUT);

/// The time limit of a leg attempt with `--leg-failover serial` when
/// another leg can follow: the budget of the leg, from the
/// limits of its steps. A slow edge in these limits is not stopped. A try
/// over more than one edge address can take longer; the limit then stops
/// it, and the next leg starts. The WARP leg has no limit.
pub(super) fn attempt_limit(kind: LegKind) -> Option<Duration> {
    match kind {
        LegKind::QuicDirect => Some(leg_budget(QUIC_TRY_BUDGET)),
        LegKind::H2Direct | LegKind::H2Relay => Some(leg_budget(crate::edge_h2::TRY_BUDGET)),
        LegKind::QuicWarp => None,
    }
}

/// Run `attempt` again when the edge refuses the registration and asks for
/// a retry (`retry: yes`): a new try starts in [`REGISTER_BUDGET`] from the
/// first refusal, after [`register_wait`]. A new quick tunnel can be "not
/// found" at the edge for some seconds after its provision; the tries keep
/// the same tunnel. With no retry, the first refusal ends the leg.
#[cfg(test)]
pub(super) async fn register_retrying<T, F, Fut>(attempt: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    register_retrying_with(&pod_core::Report::default(), attempt).await
}

/// [`register_retrying`], with the line of each retry as an event for
/// `report`.
pub(super) async fn register_retrying_with<T, F, Fut>(report: &pod_core::Report, mut attempt: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut deadline = None;
    let mut retry = 0;
    loop {
        let err = match attempt().await {
            Ok(value) => return Ok(value),
            Err(err) => err,
        };
        let Some(refused) = crate::fail::find::<rpc::Refused>(&err).filter(|r| r.should_retry).cloned() else {
            return Err(err);
        };
        let now = tokio::time::Instant::now();
        let deadline = *deadline.get_or_insert(now + REGISTER_BUDGET);
        if now >= deadline {
            return Err(err);
        }
        retry += 1;
        let wait = register_wait(refused.retry_after, retry).min(deadline - now);
        let text = format!(
            "the edge refused the registration ({}) and asks for a retry; the registration runs again in {} ms",
            refused.cause,
            wait.as_millis()
        );
        report.event(pod_core::Event::info("tunnel.register", text));
        tokio::time::sleep(wait).await;
    }
}

/// The failure of a start when each leg failed: the text of each failure,
/// the stage of the last one, and the typed cause of the last one (a
/// refusal of the registration, an error of I/O) as the cause. The words of
/// that cause are in the chain once: the text of the last leg stops before
/// them. When each leg failed with a refusal with retry: yes, the class is
/// `TempFail` (a later start can work); else `Unavailable`.
pub(super) fn no_leg_worked(plan: &[LegKind], failed: Vec<(usize, crate::fail::Failure)>) -> crate::fail::Failure {
    let retry = |err: &crate::fail::Failure| crate::fail::find::<rpc::Refused>(err).is_some_and(|r| r.should_retry);
    let class = if !failed.is_empty() && failed.iter().all(|(_, err)| retry(err)) {
        Class::TempFail
    } else {
        Class::Unavailable
    };
    let mut text: Vec<String> = Vec::with_capacity(failed.len());
    let mut last = None;
    let n = failed.len();
    for (k, (i, err)) in failed.into_iter().enumerate() {
        if k + 1 < n {
            text.push(format!("{}: {}", plan[i], err.chain()));
            continue;
        }
        let stage = err.stage;
        let (steps, root) = err.into_root();
        text.push(format!("{}: {steps}", plan[i]));
        last = Some((stage, root));
    }
    let text = format!("no edge leg worked: {}", text.join("; "));
    match last {
        Some((stage, Some(root))) => pod_core::Failure::caused(stage, class, text, root),
        Some((stage, None)) => crate::fail::new(stage, class, text),
        None => crate::fail::new(Stage::Connect, class, text),
    }
}

/// Give `attempt` the time limit `limit` (see [`attempt_limit`]). A
/// filtered port does not answer, so the next leg does not wait for the
/// end of each timeout of the transport.
pub(super) async fn bounded<T>(
    limit: Option<Duration>,
    attempt: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let Some(limit) = limit else {
        return attempt.await;
    };
    match tokio::time::timeout(limit, attempt).await {
        Ok(result) => result,
        Err(_) => bail!(Stage::Connect, Class::Timeout, "the attempt did not complete in {}s", limit.as_secs()),
    }
}

/// The edge list of the settings, which replaces edge discovery (as
/// `cloudflared --edge` does); `None` when it is not set or empty.
///
/// The edge list (podnet's `PODNET_EDGE`, a comma-separated `host:port`
/// list) is for a self-hosted or pinned edge. It is also the only way to
/// test the `auto` fallback on purpose. Set it to an address where nothing
/// answers: the direct dial then fails where discovery succeeds.
pub(crate) fn edge_override(settings: &crate::Settings) -> Option<Vec<edge::EdgeAddr>> {
    let list = settings.edge.as_ref()?;
    (!list.is_empty()).then(|| list.iter().map(|socket| edge::EdgeAddr::new(*socket)).collect())
}

/// The target of the UDP probe: the first address of the edge list of the
/// settings, else the probe address of the edge.
pub(super) fn probe_target(settings: &crate::Settings) -> std::net::SocketAddr {
    settings.edge.as_ref().and_then(|list| list.first().copied()).unwrap_or_else(crate::udp::default_probe_target)
}

/// The relay of the leg `kind`: the relay of the host for the leg
/// `http2+relay` alone, as podnet's `--transport relay`. Each other leg
/// dials the edge with no relay.
pub(super) fn relay_for(kind: LegKind, host: &Host) -> Result<Option<crate::host::EdgeRelay>> {
    match kind {
        LegKind::H2Relay => host.relay.clone().map(Some).or_fail(
            Stage::Relay,
            Class::Config,
            "the leg http2+relay needs a relay, and the consumer gives none",
        ),
        LegKind::QuicDirect | LegKind::H2Direct | LegKind::QuicWarp => Ok(None),
    }
}

pub(super) async fn direct_targets(host: &Host) -> Result<Vec<edge::EdgeAddr>> {
    if let Some(list) = edge_override(&host.settings) {
        return Ok(list);
    }
    let discovered = crate::edge_discovery::discover(host, IpVersionFilter::Auto).await.context("edge discovery")?;
    if discovered.is_empty() {
        bail!(Stage::Dns, Class::Unavailable, "edge discovery returned no addresses");
    }
    Ok(discovered)
}
