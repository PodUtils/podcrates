//! Opening a forward session through the relay, with failover.
//!
//! The hosts of a [`RelayList`] are tried in the order of the list, as the
//! mode of [`crate::failover`] says: by default the next host starts when the
//! one before it has not opened within 2 s. Each host has one attempt bounded
//! by [`HOST_DEADLINE`]: a token that the user gave, the cache's or a fresh
//! mint (re-minted once when the relay rejects a cached one), then the
//! WebSocket upgrade. A failure that another host might not share (a dead
//! host, a proxy that refuses or cannot reach one name, a relay error) moves
//! on to the next host; one that every host would repeat (a policy refusal, a
//! target name that does not resolve, a bad token that the user gave) stops
//! at once. Whole rounds repeat `rounds` times with jittered exponential
//! backoff. After them, last, the cleartext road (podssh T-326,
//! [`crate::cleartext`]) to a host whose TLS failed on the way. Each attempt
//! passes the run's gate of its host first ([`crate::gate`], podssh T-637),
//! so that the sessions of a run that a failing host meets together cost one
//! probe. With the feature `plain-ws`, `open_on_loopback` does the same
//! over plain `ws://` to stand-in relays on the loopback, for the tests of an
//! embedder.

use std::sync::Arc;
use std::time::Duration;

use pod_proxy::DialError;
use pod_ws::{ConnectError, Endpoint, RelaySession, Trust, WsClientConfig};

pub use pod_core::backoff::{BACKOFF_FIRST, BACKOFF_MAX, CONNECT_TIMEOUT, backoff};

use crate::context::Context;
use crate::failover::{self, Attempt, MintGate, Mode, SERIAL_STEP, STAGGER};
use crate::failures::{self, Records};
use crate::relay::{Relay, RelayList, url_host};
use crate::token::{self, MintContext, Origin, TokenError};

/// Bound on one whole host attempt, minting included, so a host that answers
/// slowly at every step still hands over to the next one.
pub const HOST_DEADLINE: Duration = Duration::from_secs(45);

/// Why one host attempt failed.
#[derive(Debug)]
pub enum OpenError {
    /// The settings forbid each connection (`Settings::offline`).
    Offline,
    Token(TokenError),
    Connect {
        error: ConnectError,
        origin: Origin,
    },
    /// The attempt ran past [`HOST_DEADLINE`].
    Deadline,
    /// The run's gate holds the host after a failure of one of its
    /// sessions (podssh T-637): the wait left, or `None` while a probe runs. Only
    /// where the list has a next host, which the run tries at once.
    Held {
        left: Option<Duration>,
    },
}

impl OpenError {
    /// Whether another relay host could succeed where this one failed.
    pub fn another_host_may_help(&self) -> bool {
        match self {
            OpenError::Offline => false,
            OpenError::Deadline | OpenError::Held { .. } => true,
            OpenError::Token(TokenError::BadEnvironment | TokenError::NotIssued) => false,
            OpenError::Token(_) => true,
            OpenError::Connect { error, .. } => match error {
                ConnectError::Config(_) => false,
                ConnectError::Dial(DialError::BadProxy(_) | DialError::InvalidTarget(_)) => false,
                // The proxy wants credentials: the same for every host.
                ConnectError::Dial(DialError::ProxyRefused { status: 407, .. }) => false,
                ConnectError::Dial(_)
                | ConnectError::Tls(_)
                | ConnectError::Timeout { .. }
                | ConnectError::Upgrade(_)
                | ConnectError::Http(_) => true,
                // Policy, or a token rejected even fresh: every host agrees.
                ConnectError::Refused { status: 400 | 401 | 403, .. } => false,
                ConnectError::Refused { status: 502, body } => !target_name_fails(body),
                ConnectError::Refused { .. } => true,
            },
        }
    }

    /// What to print, one message per line, with no prefix of a program.
    /// `target` is the `host:port` the session was for.
    pub fn lines(&self, target: &str) -> Vec<String> {
        match self {
            OpenError::Offline => vec!["the offline setting is on, so no connection goes anywhere".to_string()],
            OpenError::Deadline => {
                vec![format!("the relay host did not answer within {} s", HOST_DEADLINE.as_secs())]
            }
            OpenError::Held { left } => vec![format!("{}; trying the next host", crate::gate::words(*left))],
            OpenError::Token(e) => vec![e.to_string()],
            OpenError::Connect { error, origin } => {
                let mut lines = vec![error.to_string()];
                let hint = match error {
                    ConnectError::Refused { status: 400 | 403, body } if is_policy_refusal(body) => {
                        Some(format!("the relay's policy does not allow {target}"))
                    }
                    ConnectError::Refused { status: 403, .. } if *origin == Origin::Environment => {
                        Some("the relay token that was given was rejected".to_string())
                    }
                    ConnectError::Refused { status: 502, .. } => Some(format!("the relay could not reach {target}")),
                    ConnectError::Dial(DialError::ProxyRefused { status, .. }) if *status >= 500 => Some(format!(
                        "the proxy could not reach the relay (it answered {status}); another relay host may work"
                    )),
                    ConnectError::Dial(DialError::ProxyRefused { .. }) => {
                        Some("the proxy does not allow connections to the relay".to_string())
                    }
                    _ => None,
                };
                lines.extend(hint);
                lines
            }
        }
    }
}

/// The relay's wording for a policy refusal (spec: "not in the ALLOW list"),
/// defined once in `pod-ws`.
pub use pod_ws::client::is_policy_refusal;

/// Whether `e` came once a connection to the host had opened, and is no
/// answer of the relay's own (a rate, a relay that issues no token): then
/// the server at that address may be another's.
fn after_connection(e: &OpenError) -> bool {
    let opened = |c: &ConnectError| !matches!(c, ConnectError::Dial(_) | ConnectError::Config(_));
    match e {
        OpenError::Connect { error, .. } | OpenError::Token(TokenError::Connect(error)) => opened(error),
        OpenError::Token(TokenError::Failed { .. }) => true,
        _ => false,
    }
}

/// A `502` whose reason is the target's name or address, which no other host
/// will resolve differently.
fn target_name_fails(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("does not resolve")
        || body.contains("no aaaa address")
        || body.contains("no a address")
        || is_policy_refusal(&body)
}

/// What to open.
pub struct Request<'a> {
    /// The way out, the cache, the settings and the run's state.
    pub ctx: &'a Context,
    pub relays: &'a RelayList,
    /// `/connect/<host>/<port>`, with the relay's connect knobs if any.
    pub path: &'a str,
    pub trust: &'a Trust,
    /// `host:port`, for messages.
    pub target: &'a str,
    /// Rounds over the whole list (`ConnectionAttempts`); at least one.
    pub rounds: u32,
    /// How the hosts of a round are tried (podssh T-220).
    pub failover: Mode,
}

/// An open session and the host that gave it.
#[derive(Debug)]
pub struct Opened {
    pub session: RelaySession,
    pub relay: Relay,
    /// Whether the cleartext road carries it, as each hop reads (podssh T-326).
    pub cleartext: bool,
}

/// Every attempt that failed, in order, as (host, error).
#[derive(Debug)]
pub struct Failure {
    pub attempts: Vec<(String, OpenError)>,
}

impl Failure {
    /// The error the run is classified by: the last one, which stopped it.
    pub fn last(&self) -> &OpenError {
        &self.attempts.last().expect("a failure has at least one attempt").1
    }

    /// What to print. One host: its lines. Several: one line per host, then
    /// the last error's hints.
    pub fn lines(&self, target: &str) -> Vec<String> {
        if self.attempts.len() == 1 {
            return self.attempts[0].1.lines(target);
        }
        let mut out: Vec<String> = self
            .attempts
            .iter()
            .map(|(host, e)| format!("{}: {}", url_host(host), e.lines(target).first().cloned().unwrap_or_default()))
            .collect();
        out.extend(self.last().lines(target).into_iter().skip(1));
        out
    }
}

/// Open a forward session, failing over across `req.relays` as
/// `req.failover` says. `notes` receives progress worth a user's attention (a
/// host failing, a retry, a token that could not be cached), each host's in
/// the order of the list.
pub async fn open(req: &Request<'_>, notes: &mut (dyn FnMut(&str) + Send)) -> Result<Opened, Failure> {
    open_on(req, Wire::Tls, notes).await
}

/// [`open`] over plain `ws://` to stand-in relays on the loopback, for the
/// tests of an embedder and a relay on the loopback (feature `plain-ws`;
/// podnet's binary turns it on, call C101):
/// the same order of the hosts, failover, tokens and notes, with no other
/// road. A host that is not the loopback is refused before any connection,
/// as `pod_ws::plain` refuses one, so a token never leaves this host in
/// clear. A token comes from the settings, the run or the cache: a mint
/// goes over TLS, which a stand-in does not answer.
#[cfg(feature = "plain-ws")]
pub async fn open_on_loopback(req: &Request<'_>, notes: &mut (dyn FnMut(&str) + Send)) -> Result<Opened, Failure> {
    open_on(req, Wire::PlainLoopback, notes).await
}

/// The wire of each attempt of a run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    /// The roads of TLS: QUIC where it is offered, HTTP/2, HTTP/1.1.
    Tls,
    /// Plain `ws://` to the loopback.
    #[cfg(feature = "plain-ws")]
    PlainLoopback,
}

async fn open_on(req: &Request<'_>, wire: Wire, notes: &mut (dyn FnMut(&str) + Send)) -> Result<Opened, Failure> {
    let mut attempts: Vec<(String, OpenError)> = Vec::new();
    let ctx = req.ctx;
    if ctx.settings.offline {
        return Err(Failure { attempts: vec![(req.relays.primary().host.clone(), OpenError::Offline)] });
    }
    // No background probe runs beside a race (podssh T-608).
    let _racing = crate::rank_auto::racing(ctx.races());
    // The addresses that carried a session, for a resolver that fails (podssh T-344).
    crate::addresses::install(ctx);
    // A host that failed lately goes last (podssh T-059), and one that opened first (podssh T-563).
    let mut records = Records::load(&ctx.cache);
    // The lists of the net's relay members, asked first (podssh T-617), join the list that the rank places (podssh T-607).
    let refreshed = crate::pool::members::refreshed(ctx, req.relays, req.trust).await;
    let listed = crate::rank_list::placed_now(ctx, &refreshed);
    let (order, moved) = failures::order(failures::WEBSOCKET, &listed, &records, crate::pool::now_ms());
    for note in moved {
        notes(&note);
    }
    let hosts: &Vec<Relay> = &order.iter().map(|&i| listed[i].clone()).collect();
    let shared = Arc::new(Shared {
        ctx: ctx.clone(),
        path: req.path.to_string(),
        trust: req.trust.clone(),
        target: req.target.to_string(),
        primary: req.relays.primary().host.clone(),
        explicit: req.relays.explicit,
        several: hosts.len() > 1,
        samples: Default::default(),
        wire,
    });
    let gate = MintGate::default();
    let rounds = req.rounds.max(1);
    for round in 1..=rounds {
        if round > 1 {
            let wait = backoff(round - 1);
            notes(&format!("retrying in {:.1} s (round {round} of {rounds})", wait.as_secs_f32()));
            tokio::time::sleep(wait).await;
        }
        let start = |i: usize| {
            // In `serial`, a host with a successor has a short step.
            let last = i + 1 == hosts.len();
            let step = if req.failover == Mode::Serial && !last { SERIAL_STEP } else { CONNECT_TIMEOUT };
            attempt(hosts[i].clone(), shared.clone(), gate.clone(), step, !last)
        };
        // Each host held by the run's gate: the run waits at the last (podssh T-637).
        if let Some(line) = ctx.gates().all_held(hosts) {
            notes(&line);
        }
        let stops = |e: &OpenError| !e.another_host_may_help();
        let raced = failover::race(hosts.len(), req.failover, start, stops, notes).await;
        // A session that is not kept is closed at once, so that the relay
        // frees the socket of its target.
        for (_, session) in raced.dismissed {
            tokio::spawn(async move {
                let _ = session.send_close(1000, "").await;
            });
        }
        let reasons = raced.failed.iter().filter_map(|(i, e)| failures::reason(e).map(|r| (hosts[*i].host.clone(), r)));
        let silent = raced.silent.iter().map(|i| (hosts[*i].host.clone(), failures::SILENT.to_string()));
        let failed: Vec<(String, String)> = reasons.chain(silent).collect();
        let opened_host = raced.kept.as_ref().map(|(i, _)| hosts[*i].host.as_str());
        crate::rank_list::keep(ctx, &shared.samples);
        if records.update(failures::WEBSOCKET, &failed, opened_host, crate::pool::now_ms()) {
            records.store(&ctx.cache);
        }
        // A host that failed after its connection opened, as the relay itself
        // would not (TLS, the upgrade, another's refusal): an address of the
        // cache that it was reached at is not the relay's (podssh T-344).
        for (i, e) in &raced.failed {
            if failures::reason(e).is_some() && after_connection(e) {
                crate::addresses::failed(ctx, &hosts[*i].host);
            }
        }
        attempts.extend(raced.failed.into_iter().map(|(i, e)| (hosts[i].host.clone(), e)));
        if let Some((i, session)) = raced.kept {
            // The address that carried it, for a run whose resolver fails.
            crate::addresses::carried(ctx, &hosts[i].host, session.peer());
            return Ok(opened(ctx, session, hosts[i].clone(), &shared.primary, false));
        }
        if raced.stopped {
            break;
        }
    }
    // Last, the cleartext road to a host whose TLS failed on the way (podssh T-326,
    // podssh Q45); its lines are notes, and the failure stays the TLS road's.
    if wire == Wire::Tls
        && let Some((relay, session)) = crate::cleartext::last(ctx, hosts, &attempts, &shared.path, notes).await
    {
        return Ok(opened(ctx, session, relay, &shared.primary, true));
    }
    Err(Failure { attempts })
}

/// A session that opened, under the relay's own limits, as its pool's cache
/// keeps them: the run's budgets go under them, and the session takes in
/// frames up to its cap (podssh T-342).
fn opened(ctx: &Context, session: RelaySession, relay: Relay, primary: &str, cleartext: bool) -> Opened {
    let limits = crate::pool::cached_limits(&ctx.cache, primary);
    ctx.set_in_use(limits);
    session.set_receive_cap(pod_ws::frame::receive_cap(limits.max_frame_bytes));
    Opened { session, relay, cleartext }
}

/// What the attempts of one run share.
struct Shared {
    ctx: Context,
    path: String,
    trust: Trust,
    /// `host:port`, for messages.
    target: String,
    /// The first host: the key of the cached pool.
    primary: String,
    /// Whether the user named the hosts.
    explicit: bool,
    /// Whether there is more than one host, so that a failure names its own.
    several: bool,
    /// The time to open of each attempt that opened (podssh T-607).
    samples: crate::rank_list::Samples,
    wire: Wire,
}

/// One host within [`HOST_DEADLINE`], each step within `step`, with its
/// notes: a host that fails names itself when there are others. First the
/// run's gate of the host, its wait outside the deadline (podssh T-637); with a
/// `successor`, a held host is left for the next at once.
async fn attempt(
    relay: Relay,
    shared: Arc<Shared>,
    gate: MintGate,
    step: Duration,
    successor: bool,
) -> Attempt<RelaySession, OpenError> {
    let mut notes = Vec::new();
    // On the heap: the gate's future would hold the attempt's whole, the
    // largest of the opener, and a test's worker of 2 MiB overflowed.
    let tried = Box::pin(try_host(&relay, &shared, &gate, step, &mut notes));
    let result = crate::gate::guarded(shared.ctx.gates(), &relay, successor, HOST_DEADLINE, tried).await;
    if let Err(e) = &result
        && e.another_host_may_help()
        && shared.several
    {
        let first = e.lines(&shared.target).first().cloned().unwrap_or_default();
        notes.push(format!("{}: {first}", url_host(&relay.host)));
    }
    Attempt { result, notes }
}

/// One host: a token, then the upgrade. A token is read or minted in the
/// turn of its deployment ([`MintGate`]): of two attempts, the second reads
/// the token that the first cached, or mints at its own host when the first
/// is still silent after [`STAGGER`].
async fn try_host(
    relay: &Relay,
    shared: &Shared,
    gate: &MintGate,
    step: Duration,
    notes: &mut Vec<String>,
) -> Result<RelaySession, OpenError> {
    let run = &shared.ctx;
    let ctx = MintContext { ctx: run, relay, trust: &shared.trust, timeout: step };
    let key = token::token_key(relay);
    let mut token = {
        let _turn = gate.hold(&key, STAGGER).await;
        token_for(shared, &ctx).await.map_err(OpenError::Token)?
    };
    if let Some(why) = token.cache_warning.take() {
        notes.push(format!("the relay token could not be cached, so one is minted per run: {why}"));
    }
    if let Some(note) = token.cache_note.take() {
        notes.push(note);
    }
    if token.origin == Origin::Minted && !shared.explicit {
        spawn_pool_refresh(run.clone(), relay.clone(), shared.primary.clone(), shared.trust.clone());
    }
    let config = WsClientConfig {
        endpoint: Endpoint { host: relay.host.clone(), port: relay.port, path: shared.path.clone() },
        trust: shared.trust.clone(),
        server_name: relay.host.clone(),
        timeout: step,
        idle_timeout: Some(pod_ws::DEFAULT_IDLE_TIMEOUT),
        way: run.way.clone(),
        limits: pod_ws::Limits::RELAY,
    };
    let mut started = std::time::Instant::now();
    let mut result = connect(shared, &config, token.secret()).await;
    // A cached token the relay no longer accepts: forget it and mint once.
    // Not for a policy refusal, which a new token cannot fix.
    if let Err(ConnectError::Refused { status: 403, body }) = &result
        && token.origin == Origin::Cache
        && !is_policy_refusal(body)
        && shared.wire == Wire::Tls
    {
        token::forget(run, &key);
        token = {
            let _turn = gate.hold(&key, STAGGER).await;
            token::obtain(&ctx, true).await.map_err(OpenError::Token)?
        };
        started = std::time::Instant::now();
        result = connect(shared, &config, token.secret()).await;
    }
    if result.is_ok() {
        crate::rank_list::sample(&shared.samples, relay, started.elapsed());
    }
    let origin = token.origin;
    drop(token);
    result.map_err(|error| OpenError::Connect { error, origin })
}

/// The token of one attempt: on the plain wire, one that the run holds, as
/// a stand-in relay mints none.
async fn token_for(shared: &Shared, ctx: &MintContext<'_>) -> Result<token::Token, TokenError> {
    match shared.wire {
        Wire::Tls => token::obtain(ctx, false).await,
        #[cfg(feature = "plain-ws")]
        Wire::PlainLoopback => plain::token(ctx),
    }
}

/// The upgrade at one host on the wire of the run.
async fn connect(shared: &Shared, config: &WsClientConfig, token: &str) -> Result<RelaySession, ConnectError> {
    match shared.wire {
        Wire::Tls => crate::quic_road::connect(&shared.ctx, config, token, shared.explicit).await,
        #[cfg(feature = "plain-ws")]
        Wire::PlainLoopback => plain::connect(config, token).await,
    }
}

/// Refresh the cached pool in the background after a mint: a stale or missing
/// pool only means fewer alternates, so failure is silent and the session
/// never waits for it.
fn spawn_pool_refresh(ctx: Context, relay: Relay, primary: String, trust: Trust) {
    if !crate::pool::needs_refresh(&ctx.cache, &primary, crate::pool::now_ms()) {
        return;
    }
    tokio::spawn(async move {
        let _ = crate::pool::refresh(&ctx, &relay, &primary, &trust, CONNECT_TIMEOUT).await;
    });
}

#[cfg(feature = "plain-ws")]
#[path = "open_plain.rs"]
mod plain;

#[cfg(test)]
#[path = "open_tests.rs"]
mod tests;
