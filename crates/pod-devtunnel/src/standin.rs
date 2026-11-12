//! A stand-in of Dev Tunnels on the loopback, for the tests of this crate
//! and of a consumer (feature `standin`): one listener that answers the
//! management API's get of a tunnel and the relay's WebSocket upgrade, and
//! behind the upgrade an SSH server (russh) as the host of the tunnel. It
//! checks what the vendor's service checks (the user's token, the scope,
//! the version, the subprotocol, the latest connect token, the user with
//! no authentication) and keeps what it saw ([`Seen`]). Its tokens are
//! made-up values.

mod host;

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use pod_proxy::http::request::{self, Request};
use pod_ws::server;
use russh::keys::{Algorithm, PrivateKey};
use russh::{MethodKind, MethodSet};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use host::Port;

use crate::service;

/// The path of the client relay URI of the stand-in, before the tunnel id.
pub const RELAY_PATH: &str = "/api/v1/Client/Connect/";
/// The start of each connect token that the stand-in mints.
pub const TOKEN_PREFIX: &str = "standin-connect-";

/// The host keys that the connect information of the stand-in gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    /// The key of its SSH server.
    Host,
    /// Another key: a client must refuse the session.
    Other,
    /// None.
    Empty,
}

/// What the stand-in serves.
#[derive(Clone, Debug)]
pub struct Plan {
    pub name: String,
    pub id: String,
    pub cluster: String,
    /// The value of `Authorization` that the management API takes.
    pub authorization: String,
    pub ports: Vec<(u16, Port)>,
    pub keys: Keys,
    /// The answer holds no relay endpoint (no host is connected).
    pub no_host: bool,
}

impl Plan {
    /// A tunnel `standin-box` (id `bcd123`, cluster `usw2`) with an echo on
    /// port 22, for the user token `user` of `scheme`.
    pub fn echo(scheme: &str, user: &str) -> Plan {
        Plan {
            name: "standin-box".into(),
            id: "bcd123".into(),
            cluster: "usw2".into(),
            authorization: format!("{scheme} {user}"),
            ports: vec![(22, Port::Echo)],
            keys: Keys::Host,
            no_host: false,
        }
    }
}

/// What the stand-in saw. No token is kept: only whether it was the right
/// one.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// The request-target of each get of the management API.
    pub gets: Vec<String>,
    /// For each get: the user's token was the plan's.
    pub user_ok: Vec<bool>,
    /// For each upgrade: the subprotocols that it offered.
    pub subprotocols: Vec<String>,
    /// For each upgrade: `fresh` (the last connect token), `stale` (an
    /// earlier one), `wrong` or `none`.
    pub relay_tokens: Vec<&'static str>,
    /// The user of each authentication.
    pub users: Vec<String>,
    /// The host, the port, the originator and its port of each
    /// `direct-tcpip` channel.
    pub channels: Vec<(String, u32, String, u32)>,
}

pub(crate) struct State {
    plan: Plan,
    seen: Mutex<Seen>,
    minted: AtomicU64,
    refuse: AtomicUsize,
    refuse_user: AtomicBool,
    epoch: watch::Sender<u64>,
    key: PrivateKey,
    other: PrivateKey,
    port: u16,
}

impl State {
    fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The stand-in on its loopback port. It stops when it is dropped.
pub struct Standin {
    state: Arc<State>,
    accept: JoinHandle<()>,
}

impl Drop for Standin {
    fn drop(&mut self) {
        self.accept.abort();
        self.state.epoch.send_modify(|e| *e += 1);
    }
}

fn key() -> io::Result<PrivateKey> {
    PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).map_err(io::Error::other)
}

impl Standin {
    /// Bind `127.0.0.1:0` and serve `plan`.
    pub async fn start(plan: Plan) -> io::Result<Standin> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(State {
            plan,
            seen: Mutex::new(Seen::default()),
            minted: AtomicU64::new(0),
            refuse: AtomicUsize::new(0),
            refuse_user: AtomicBool::new(false),
            epoch: watch::channel(0).0,
            key: key()?,
            other: key()?,
            port,
        });
        let served = state.clone();
        let accept = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(serve(served.clone(), tcp));
            }
        });
        Ok(Standin { state, accept })
    }

    /// The base of the management API: `http://127.0.0.1:PORT`.
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.state.port)
    }

    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.state.port))
    }

    /// What the stand-in saw so far.
    pub fn seen(&self) -> Seen {
        self.state.seen().clone()
    }

    /// The connect tokens minted so far.
    pub fn minted(&self) -> u64 {
        self.state.minted.load(Ordering::SeqCst)
    }

    /// End each relay session now, as a relay that goes away does.
    pub fn drop_relays(&self) {
        self.state.epoch.send_modify(|e| *e += 1);
    }

    /// Refuse the next `n` upgrades with 503.
    pub fn refuse_upgrades(&self, n: usize) {
        self.state.refuse.store(n, Ordering::SeqCst);
    }

    /// Refuse the user's token from now on, as for a token that expired.
    pub fn refuse_user(&self) {
        self.state.refuse_user.store(true, Ordering::SeqCst);
    }

    /// The host key of the SSH server, as `hostPublicKeys` gives it.
    pub fn host_key(&self) -> String {
        crate::ssh::key_text(self.state.key.public_key()).unwrap_or_default()
    }
}

/// One connection: a get of the management API, or an upgrade of the
/// relay.
async fn serve(state: Arc<State>, mut tcp: TcpStream) {
    let Ok((head, rest)) = request::read_request(&mut tcp, Vec::new()).await else { return };
    if server::asks_upgrade(&head) {
        relay(state, tcp, head, rest).await;
        return;
    }
    let answer = management(&state, &head);
    let _ = tcp.write_all(&answer).await;
    let _ = tcp.shutdown().await;
}

/// A problem document of `status`.
fn problem(status: u16, title: &str) -> Vec<u8> {
    let body = serde_json::json!({ "title": title, "status": status }).to_string();
    request::answer(status, "application/problem+json", body.as_bytes(), false)
}

/// The answer of the management API to `head`.
fn management(state: &State, head: &Request) -> Vec<u8> {
    let plan = &state.plan;
    let user_ok =
        head.header("authorization") == Some(plan.authorization.as_str()) && !state.refuse_user.load(Ordering::SeqCst);
    {
        let mut seen = state.seen();
        seen.gets.push(head.target.clone());
        seen.user_ok.push(user_ok);
    }
    let (path, query) = head.target.split_once('?').unwrap_or((&head.target, ""));
    let Some(ident) = path.strip_prefix(service::tunnels_path()).and_then(|p| p.strip_prefix('/')) else {
        return problem(404, "Not Found");
    };
    if head.method != "GET" {
        return problem(405, "Method Not Allowed");
    }
    if !user_ok {
        return problem(401, "Unauthorized");
    }
    let pairs: Vec<&str> = query.split('&').collect();
    let scope = format!("tokenScopes={}", service::token_scope());
    let version = format!("api-version={}", service::api_version());
    if !pairs.contains(&scope.as_str()) || !pairs.contains(&version.as_str()) {
        return problem(400, "Bad Request");
    }
    if ident != plan.name && ident != plan.id {
        return problem(404, "Not Found");
    }
    let minted = state.minted.fetch_add(1, Ordering::SeqCst) + 1;
    let keys = match plan.keys {
        Keys::Host => vec![crate::ssh::key_text(state.key.public_key()).unwrap_or_default()],
        Keys::Other => vec![crate::ssh::key_text(state.other.public_key()).unwrap_or_default()],
        Keys::Empty => Vec::new(),
    };
    let relay = format!("ws://127.0.0.1:{}{RELAY_PATH}{}", state.port, plan.id);
    let endpoints = if plan.no_host {
        serde_json::json!([])
    } else {
        serde_json::json!([{
            "connectionMode": "TunnelRelay",
            "hostId": "standin-host",
            "hostPublicKeys": keys,
            "clientRelayUri": relay,
            "hostRelayUri": format!("ws://127.0.0.1:{}/api/v1/Host/Connect/{}", state.port, plan.id),
        }])
    };
    let body = serde_json::json!({
        "clusterId": plan.cluster,
        "tunnelId": plan.id,
        "name": plan.name,
        "accessTokens": { service::token_scope(): format!("{TOKEN_PREFIX}{minted}") },
        "endpoints": endpoints,
    });
    request::answer(200, "application/json", body.to_string().as_bytes(), false)
}

/// The relay: the checks of the upgrade, the 101 with the subprotocol,
/// then the SSH server over the frames until the end of the session or a
/// [`Standin::drop_relays`].
async fn relay(state: Arc<State>, mut tcp: TcpStream, head: Request, rest: Vec<u8>) {
    let mut epoch = state.epoch.subscribe();
    let offered = head.header("sec-websocket-protocol").unwrap_or("").to_string();
    let latest = format!("{} {TOKEN_PREFIX}{}", service::relay_scheme(), state.minted.load(Ordering::SeqCst));
    let token = match head.header("authorization") {
        None => "none",
        Some(value) if value == latest => "fresh",
        Some(value) if value.starts_with(&format!("{} {TOKEN_PREFIX}", service::relay_scheme())) => "stale",
        Some(_) => "wrong",
    };
    {
        let mut seen = state.seen();
        seen.subprotocols.push(offered.clone());
        seen.relay_tokens.push(token);
    }
    let refused = state.refuse.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok();
    let key = match server::check_upgrade(&head) {
        Ok(key) => key,
        Err(no) => {
            let _ = tcp.write_all(&request::refusal(no.status, &no.why, &no.headers)).await;
            return;
        }
    };
    let protocol = service::subprotocol();
    let refusal = if refused {
        Some((503, "the relay is busy"))
    } else if !offered.split(',').any(|p| p.trim() == protocol) {
        Some((400, "no subprotocol of a client"))
    } else if token != "fresh" {
        Some((401, "the connect token is not the tunnel's"))
    } else {
        None
    };
    if let Some((status, words)) = refusal {
        let _ = tcp.write_all(&request::refusal(status, words, &[])).await;
        return;
    }
    let switching = String::from_utf8_lossy(&server::switching(&key)).into_owned();
    let answer = switching.replacen("\r\n\r\n", &format!("\r\nSec-WebSocket-Protocol: {protocol}\r\n\r\n"), 1);
    if tcp.write_all(answer.as_bytes()).await.is_err() {
        return;
    }
    let session = server::session(tcp, rest, None);
    let (ssh_io, ws_io) = tokio::io::duplex(64 * 1024);
    let config = Arc::new(russh::server::Config {
        keys: vec![state.key.clone()],
        methods: MethodSet::from(&[MethodKind::None][..]),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..russh::server::Config::default()
    });
    let handler = host::Host { state: state.clone() };
    let ssh = tokio::spawn(async move {
        if let Ok(running) = russh::server::run_stream(config, ssh_io, handler).await {
            let _ = running.await;
        }
    });
    tokio::select! {
        _ = pod_ws::bridge(session, ws_io, &pod_ws::NoTally) => {}
        _ = epoch.changed() => {}
    }
    ssh.abort();
}
