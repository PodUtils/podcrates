//! The pairs of the reverse road (feature `pair`): `POST /v1/pair` makes one,
//! `GET /v1/status/<name>` says whether its node is online, and
//! `POST /v1/stop/<name>` ends it. They live on the relay's control host
//! only; no pool host was measured to serve `/v1/*`.
//!
//! A pair is three tokens, and each is a credential: in memory they are wiped
//! on drop, on disk they are in a private file, and no output, error or
//! `Debug` shows one. The relay reads them from `X-Relay-Token` only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_ws::Trust;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::cache::{self, Cache, MIN_REMAINING_MS, valid_token};
use crate::context::Context;
use crate::relay::{Relay, check_node_name, parse_relay};
use crate::token::relay_name;

/// Why a pairing call failed.
mod error;
/// A node's pair from the parts that its user gives (podvm).
pub mod given;
/// A pair that its owner halts and resumes in podssh's relay role (podssh T-647).
pub mod halt;
/// A leg to a member's node at a member's relay role, by its key (podssh T-434).
pub mod leg;
/// A pair whose connect token takes one upgrade alone (podssh T-226).
pub mod once;
/// The operator's part kept under a label on an operator's host (podssh T-086).
pub mod part;
/// A pair that its owner renews in podssh's relay role, and its end (podssh T-592).
pub mod renew;
/// A pair's stop token, rotated in podssh's relay role (podssh T-615).
pub mod rotate;
/// The labels of the store, and the pair with the most time left (podnet).
pub mod stored;

pub use error::PairError;
pub(crate) use error::{check_label, not_made, refused};
pub use stored::{labels, labels_from, newest, newest_from};

/// A pair, as `POST /v1/pair` gives it.
pub struct Pair {
    /// The control host that made the pair.
    pub relay: Relay,
    /// The relay's name of the pair, in `/v1/node/<name>` and
    /// `/v1/connect/<name>`.
    pub name: String,
    node_token: Zeroizing<String>,
    connect_token: Zeroizing<String>,
    stop_token: Zeroizing<String>,
    /// When the relay forgets the pair, in milliseconds since the epoch.
    pub expires_ms: i64,
    /// The relay said that its connect token takes one upgrade alone
    /// (`"once": true`): the store keeps the mark, so that a `403` of a spent
    /// token is named as such.
    pub once: bool,
}

impl Pair {
    /// For the node's socket, `/v1/node/<name>`, and nothing else.
    pub fn node_token(&self) -> &str {
        &self.node_token
    }

    /// For the operator's socket, `/v1/connect/<name>`, and for the status.
    pub fn connect_token(&self) -> &str {
        &self.connect_token
    }

    /// For `/v1/stop/<name>`. Never printed.
    pub fn stop_token(&self) -> &str {
        &self.stop_token
    }

    /// A second copy, for a runner that owns its pair while the caller keeps
    /// its own. Each copy zeroes its tokens when it is dropped.
    #[cfg(feature = "blocking")]
    pub(crate) fn duplicate(&self) -> Pair {
        Pair {
            relay: self.relay.clone(),
            name: self.name.clone(),
            node_token: self.node_token.clone(),
            connect_token: self.connect_token.clone(),
            stop_token: self.stop_token.clone(),
            expires_ms: self.expires_ms,
            once: self.once,
        }
    }
}

impl std::fmt::Debug for Pair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pair")
            .field("relay", &self.relay)
            .field("name", &self.name)
            .field("tokens", &"<redacted>")
            .field("expires_ms", &self.expires_ms)
            .field("once", &self.once)
            .finish()
    }
}

/// Where and how to ask the control host.
pub struct PairContext<'a> {
    /// The way out, the cache and the settings.
    pub ctx: &'a Context,
    pub relay: &'a Relay,
    pub trust: &'a Trust,
    pub timeout: Duration,
}

#[derive(Deserialize)]
struct PairAnswer {
    name: String,
    node_token: String,
    connect_token: String,
    stop_token: String,
    /// Milliseconds since the epoch.
    expires: i64,
}

/// A pair from the body of a 2xx answer of `/v1/pair`: the name as podssh T-076
/// checks names, each token's shape, and at least 10 minutes left.
pub fn parse(relay: &Relay, body: &[u8], now_ms: i64) -> Result<Pair, PairError> {
    let answer: PairAnswer = serde_json::from_slice(body).map_err(|_| PairError::BadAnswer("not the expected JSON"))?;
    let PairAnswer { name, node_token, connect_token, stop_token, expires } = answer;
    let (node_token, connect_token, stop_token) =
        (Zeroizing::new(node_token), Zeroizing::new(connect_token), Zeroizing::new(stop_token));
    check_node_name(&name).map_err(|_| PairError::BadAnswer("the name could change the relay path"))?;
    if ![&node_token, &connect_token, &stop_token].iter().all(|t| valid_token(t)) {
        return Err(PairError::BadAnswer("a token does not have the shape of a token"));
    }
    let left_ms = expires.saturating_sub(now_ms);
    if left_ms < MIN_REMAINING_MS {
        return Err(PairError::ShortLived { left_ms });
    }
    let once = once::said_once(body);
    Ok(Pair { relay: relay.clone(), name, node_token, connect_token, stop_token, expires_ms: expires, once })
}

/// `POST /v1/pair` with `{}`, and once more with this client's key when the
/// relay asks for it (podssh T-448). A `409` means "pair again", which is done once.
pub async fn create(ctx: &PairContext<'_>) -> Result<Pair, PairError> {
    once::create_with(ctx, false).await.map(|(pair, _)| pair)
}

/// Whether the pair's node is online, and its sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Presence {
    pub online: bool,
    pub sessions: u64,
}

/// `GET /v1/status/<name>`, with `connect_token`: the one token that the relay
/// accepts there (as measured; the other two get `403`). The
/// operator's part holds it, so an operator's host asks too (podssh T-591).
pub async fn status(ctx: &PairContext<'_>, part: &OperatorPart) -> Result<Presence, PairError> {
    renew::status(ctx, part).await.map(|status| status.presence())
}

/// What `/v1/stop/<name>` answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Stopped {
    /// `false` when no node was online; the credentials end either way.
    pub stopped: bool,
    pub sessions: u64,
}

/// `POST /v1/stop/<name>` with `stop_token`. The relay ends the pair's
/// credentials whatever it answers (`{"stopped": false}` was seen to end
/// them), so a caller forgets its copies whatever the result: see [`revoke`].
pub async fn stop(ctx: &PairContext<'_>, pair: &Pair) -> Result<Stopped, PairError> {
    let path = format!("/v1/stop/{}", checked(&pair.name)?);
    let response = with_token(ctx, "POST", &path, pair.stop_token()).await?;
    match response.status {
        200..=299 => {
            serde_json::from_slice(&response.body).map_err(|_| PairError::BadAnswer("not the expected answer to stop"))
        }
        status => Err(refused(status, &response)),
    }
}

/// Stop the pair stored under `label`, and delete the local copy whatever the
/// relay answered. When the relay could not be reached, it answered nothing:
/// the copy is kept, because its `stop_token` is the only way to stop the
/// pair before it expires.
pub async fn revoke(ctx: &PairContext<'_>, label: &str) -> Result<Stopped, PairError> {
    let cache = &ctx.ctx.cache;
    let pair = load(cache, label)?.ok_or_else(|| PairError::BadLabel(format!("no pair is stored under {label:?}")))?;
    let result = stop(ctx, &pair).await;
    if !matches!(result, Err(PairError::Connect(_))) {
        remove(cache, label)?;
    }
    result
}

async fn with_token(
    ctx: &PairContext<'_>,
    method: &str,
    path: &str,
    token: &str,
) -> Result<pod_proxy::http::Response, PairError> {
    pod_ws::client::https_with_token(
        &ctx.ctx.way,
        method,
        &ctx.relay.host,
        ctx.relay.port,
        path,
        b"{}",
        token,
        16 * 1024,
        ctx.trust,
        ctx.timeout,
    )
    .await
    .map_err(PairError::Connect)
}

fn checked(name: &str) -> Result<&str, PairError> {
    check_node_name(name).map(|()| name).map_err(PairError::BadLabel)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// A pair as it is written: borrowed, so the tokens are not copied.
#[derive(Serialize)]
struct StoredRef<'a> {
    relay: String,
    name: &'a str,
    node_token: &'a str,
    connect_token: &'a str,
    stop_token: &'a str,
    expires: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    once: bool,
}

/// A pair as it is read; its tokens go into `Zeroizing` at once.
#[derive(Deserialize)]
struct Stored {
    relay: String,
    name: String,
    node_token: String,
    connect_token: String,
    stop_token: String,
    expires: i64,
    #[serde(default)]
    once: bool,
}

/// The operator's part of a pair, as its file holds it: what an operator
/// needs to connect, and no token of the node's or of the stop.
#[derive(Serialize)]
struct OperatorPartRef<'a> {
    relay: String,
    name: &'a str,
    connect_token: &'a str,
    expires: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    once: bool,
}

/// The operator's part, as it is read back; a whole pair has these fields too.
#[derive(Deserialize)]
struct OperatorStored {
    relay: String,
    name: String,
    connect_token: String,
    expires: i64,
    #[serde(default)]
    once: bool,
}

/// The operator's part of a pair: the relay, the pair's name and the connect
/// token, for `/v1/connect/<name>` and the status. No token of the node's or
/// of the stop.
pub struct OperatorPart {
    pub relay: Relay,
    pub name: String,
    connect_token: Zeroizing<String>,
    pub expires_ms: i64,
    /// The connect token takes one upgrade alone ([`Pair::once`]).
    pub once: bool,
}

impl OperatorPart {
    /// The operator's part of `pair`.
    pub fn of(pair: &Pair) -> OperatorPart {
        OperatorPart {
            relay: pair.relay.clone(),
            name: pair.name.clone(),
            connect_token: pair.connect_token.clone(),
            expires_ms: pair.expires_ms,
            once: pair.once,
        }
    }

    pub fn connect_token(&self) -> &str {
        &self.connect_token
    }
}

impl std::fmt::Debug for OperatorPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorPart")
            .field("relay", &self.relay)
            .field("name", &self.name)
            .field("connect_token", &"<redacted>")
            .field("expires_ms", &self.expires_ms)
            .field("once", &self.once)
            .finish()
    }
}

/// The operator's part in a file: one that `write_operator_file` wrote, or a
/// whole pair, whose connect token alone is taken. A private file, as for
/// [`read_file`].
pub fn read_operator_file(path: &Path) -> Result<OperatorPart, PairError> {
    let text = Zeroizing::new(cache::read_private(path).map_err(PairError::Store)?);
    let what = format!("the file {}", path.display());
    let stored: OperatorStored = serde_json::from_str(&text)
        .map_err(|_| PairError::Store(format!("{what} holds no operator's part of a pair")))?;
    let OperatorStored { relay, name, connect_token, expires, once } = stored;
    let connect_token = Zeroizing::new(connect_token);
    check_node_name(&name).map_err(|_| PairError::Store(format!("{what} names no pair")))?;
    if !valid_token(&connect_token) {
        return Err(PairError::Store(format!("{what} holds no usable connect token")));
    }
    let relay = parse_relay(&relay).map_err(|_| PairError::Store(format!("{what} names no relay")))?;
    Ok(OperatorPart { relay, name, connect_token, expires_ms: expires, once })
}

/// `pair-<label>.json`. A label is the user's name for a pair (podssh T-083); it must
/// be a name that podssh T-076 accepts, so it cannot leave the cache directory.
pub fn file_name(label: &str) -> Result<String, PairError> {
    check_label(label)?;
    Ok(format!("pair-{label}.json"))
}

/// Keep `pair` under `label`, in the first cache directory that takes a
/// private file.
pub fn store(cache: &Cache, label: &str, pair: &Pair) -> Result<PathBuf, PairError> {
    store_in_first(cache.dirs(), label, pair)
}

/// [`store`] over explicit directories (for tests).
pub fn store_in_first(dirs: &[PathBuf], label: &str, pair: &Pair) -> Result<PathBuf, PairError> {
    let name = file_name(label)?;
    let stored = StoredRef {
        relay: relay_name(&pair.relay),
        name: &pair.name,
        node_token: pair.node_token(),
        connect_token: pair.connect_token(),
        stop_token: pair.stop_token(),
        expires: pair.expires_ms,
        once: pair.once,
    };
    let body = Zeroizing::new(serde_json::to_vec(&stored).map_err(|e| PairError::Store(e.to_string()))?);
    cache::store_file_in_first(dirs, &name, &body).map_err(PairError::Store)
}

/// The pair kept under `label`, if any, also in a place passed over (podssh T-593).
/// Its expiry is not checked here: a caller says so with the remedy.
pub fn load(cache: &Cache, label: &str) -> Result<Option<Pair>, PairError> {
    load_from(&cache.read_dirs(), label)
}

/// [`load`] over explicit directories (for tests).
pub fn load_from(dirs: &[PathBuf], label: &str) -> Result<Option<Pair>, PairError> {
    let Some(text) = cache::load_file_from(dirs, &file_name(label)?) else { return Ok(None) };
    parse_stored(&Zeroizing::new(text), &format!("the pair stored under {label:?}")).map(Some)
}

/// A pair from a file of the store's form, as a user names one: a regular
/// file of this user that nobody else can read (`podssh node --pair-file`).
pub fn read_file(path: &Path) -> Result<Pair, PairError> {
    let text = Zeroizing::new(cache::read_private(path).map_err(PairError::Store)?);
    parse_stored(&text, &format!("the pair in {}", path.display()))
}

/// A pair as the store writes it; `what` names it in an error, with no token.
fn parse_stored(text: &str, what: &str) -> Result<Pair, PairError> {
    let stored: Stored = serde_json::from_str(text).map_err(|_| {
        // The operator's part has the connect token alone: a node needs the
        // whole pair.
        let operator_part = serde_json::from_str::<serde_json::Value>(text)
            .is_ok_and(|v| v.get("connect_token").is_some() && v.get("node_token").is_none());
        PairError::Store(if operator_part {
            format!("{what} is the operator's part of a pair, with no node token")
        } else {
            format!("{what} is not readable")
        })
    })?;
    let Stored { relay, name, node_token, connect_token, stop_token, expires, once } = stored;
    let (node_token, connect_token, stop_token) =
        (Zeroizing::new(node_token), Zeroizing::new(connect_token), Zeroizing::new(stop_token));
    let relay = parse_relay(&relay).map_err(|_| PairError::Store(format!("{what} names no relay")))?;
    Ok(Pair { relay, name, node_token, connect_token, stop_token, expires_ms: expires, once })
}

/// Forget the pair kept under `label`, in each directory where it is ours, a
/// place passed over too, where it would be read again.
pub fn remove(cache: &Cache, label: &str) -> Result<(), PairError> {
    remove_from(&cache.read_dirs(), label)
}

/// [`remove`] over explicit directories (for tests).
pub fn remove_from(dirs: &[PathBuf], label: &str) -> Result<(), PairError> {
    cache::remove_named_from(dirs, &file_name(label)?);
    Ok(())
}

/// Write the operator's part of `pair` to `path`, a new file that only its
/// owner can read (mode 0600). An existing file is never replaced.
pub fn write_operator_file(path: &Path, pair: &Pair) -> std::io::Result<()> {
    let part = OperatorPartRef {
        relay: relay_name(&pair.relay),
        name: &pair.name,
        connect_token: pair.connect_token(),
        expires: pair.expires_ms,
        once: pair.once,
    };
    let body = Zeroizing::new(serde_json::to_vec_pretty(&part).map_err(std::io::Error::other)?);
    let mut file = cache::create_new_private(path)?;
    std::io::Write::write_all(&mut file, &body)?;
    file.sync_all()
}
