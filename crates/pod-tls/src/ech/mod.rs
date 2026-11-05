//! Encrypted Client Hello (feature `ech`): the name of a host that
//! publishes a list in its HTTPS record goes sealed inside the ClientHello,
//! and the hello in the clear names the list's public name
//! (`cloudflare-ech.com` in Cloudflare's), so that what reads hellos on the
//! way learns that alone.
//!
//! **Only when the consumer asks for it** ([`Ech::set_mode`], from its own
//! variable or settings, read with [`Mode::parse`]): the record is read
//! before the connection, which behind a proxy no resolver makes otherwise.
//! Then, for each TLS connection opened by a name ([`open`]):
//!
//! 1. the name's list, kept in the [`Cache`], else read through the
//!    caller's [`Records`] (DNS over HTTPS through the same way out), once
//!    for each name and TTL;
//! 2. a hello sealed with it; a server that refuses it with retry configs
//!    gets one more, with those, which then replace the list;
//! 3. a refusal with none, or any other failure of a sealed hello, cools the
//!    name for 5 minutes and goes on with a plain hello, as before; `strict`
//!    sends no plain hello.
//!
//! What it hides is the name alone. A proxy is given the name in its
//! CONNECT, as it must be to know where to go; the address, the times and
//! the sizes stay as they were; a resolver that lies defeats it.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};

use pod_core::{BoxFuture, BoxedIo, Failure};
use tokio::time::Instant;

use crate::context::Context;
use crate::trust::Trust;

pub mod cache;
mod config;
mod road;

pub use cache::{Cache, Clock, Lookup, Why};
pub use road::{READ_WITHIN, Route, SEALED_WAIT};

/// A TLS stream that [`open`] gives, over the stream of the caller's dial.
pub type TlsStream = tokio_rustls::client::TlsStream<BoxedIo>;

/// Whether the connections seal the names that publish a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Each hello names its host, as before: the default.
    #[default]
    Off,
    /// A name that publishes a list goes sealed; else, and after a refusal,
    /// a plain hello.
    On,
    /// As `On`, but no plain hello ever goes to a name that was to be sealed:
    /// the connection fails instead, with why.
    Strict,
}

impl Mode {
    /// The word of the consumer's variable or setting (podssh: `PODSSH_ECH`,
    /// or the settings file's `ECH`): `None` for none or an empty one;
    /// another word is refused, with the words that it takes.
    pub fn parse(value: Option<&str>) -> Result<Option<Mode>, String> {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") => Ok(None),
            Some("no" | "off" | "false" | "0") => Ok(Some(Mode::Off)),
            Some("yes" | "on" | "true" | "1") => Ok(Some(Mode::On)),
            Some("strict") => Ok(Some(Mode::Strict)),
            Some(_) => Err("give yes, no or strict".into()),
        }
    }

    /// The word, as the variable takes it.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Off => "no",
            Mode::On => "yes",
            Mode::Strict => "strict",
        }
    }
}

/// The mode of `set`, else of the variable (its [`Mode::parse`]); a word of
/// the variable that is none of its own is off here, as a command refuses
/// it before, and a library's caller gets what it set. The rule of podssh,
/// for a consumer that keeps it.
pub fn chosen(set: Option<Mode>, env: Result<Option<Mode>, String>) -> Mode {
    set.or_else(|| env.ok().flatten()).unwrap_or_default()
}

/// A name's list, as its HTTPS record gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// The ECHConfigList, as the record holds it.
    pub list: Vec<u8>,
    /// The TTL of the answer, when it gave one.
    pub ttl: Option<u32>,
}

/// The read of a name's HTTPS record, which the caller gives: `pod-dns`
/// reads it by DNS over HTTPS, through the way out of the connection.
pub trait Records: Send + Sync {
    /// The ECHConfigList of `name`'s HTTPS record, before `deadline`;
    /// `None` when the record holds none; why when it could not be read.
    fn ech_list<'a>(&'a self, name: &'a str, deadline: Instant) -> BoxFuture<'a, Result<Option<Published>, String>>;
}

/// No reader: each read fails, so each name goes plain (and `strict`
/// refuses it).
pub struct NoRecords;

impl Records for NoRecords {
    fn ech_list<'a>(&'a self, _name: &'a str, _deadline: Instant) -> BoxFuture<'a, Result<Option<Published>, String>> {
        Box::pin(async { Err("no reader of HTTPS records was given".to_string()) })
    }
}

/// What ECH did for a name's last connection, as a doctor says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The server opened the sealed hello; `retried` when it took the retry
    /// configs that it sent to the first.
    Sealed { retried: bool },
    /// The name publishes no list: the hello was plain.
    NotPublished,
    /// The read of the name's record failed: the hello was plain.
    ReadFailed(String),
    /// The sealed hello failed, and why: the plain hello went next.
    Refused(String),
    /// Within 5 minutes of a refusal: the hello was plain, and nothing read.
    Cooled,
    /// `strict`: no plain hello went, and the connection failed.
    Strict(String),
}

impl Outcome {
    /// The outcome in words; `strict` is how the user asks for strict ECH
    /// ([`crate::Words::ech_strict`], podssh's `PODSSH_ECH=strict`).
    pub fn words(&self, strict: &str) -> String {
        match self {
            Outcome::Sealed { retried: false } => "the name went sealed, and the server opened it".into(),
            Outcome::Sealed { retried: true } => {
                "the name went sealed with the retry configs of the server, which opened it".into()
            }
            Outcome::Strict(why) => format!("{why}; with {strict} no plain hello went"),
            other => format!("{}, so a plain hello named it", other.reason()),
        }
    }

    /// Why the name did not go sealed, with no word of what came after.
    fn reason(&self) -> String {
        match self {
            Outcome::Sealed { .. } => "the name went sealed".into(),
            Outcome::NotPublished => "the name publishes no list".into(),
            Outcome::ReadFailed(why) => format!("the read of its HTTPS record failed ({why})"),
            Outcome::Refused(why) => format!("the sealed hello failed ({why})"),
            Outcome::Cooled => "its sealed hello failed within the last 5 minutes".into(),
            Outcome::Strict(why) => why.clone(),
        }
    }

    /// Whether the name went sealed.
    pub fn sealed(&self) -> bool {
        matches!(self, Outcome::Sealed { .. })
    }
}

/// The names whose outcome is kept; past it, the oldest goes.
const OUTCOMES: usize = 64;

/// The state of ECH in a [`Context`]: the mode that the consumer set, the
/// lists of the names, the sealed configurations, and the outcome of each
/// name.
pub struct Ech {
    mode: RwLock<Mode>,
    cache: Cache,
    configs: config::Configs,
    outcomes: Mutex<Vec<(String, Outcome)>>,
}

impl Default for Ech {
    fn default() -> Self {
        Ech::new()
    }
}

impl Ech {
    /// ECH off, with a cache on the system's clock.
    pub fn new() -> Ech {
        Ech {
            mode: RwLock::new(Mode::Off),
            cache: Cache::new(),
            configs: config::Configs::default(),
            outcomes: Mutex::new(Vec::new()),
        }
    }

    /// Set the mode: a consumer does, from its variable, then its settings.
    pub fn set_mode(&self, mode: Mode) {
        *self.mode.write().unwrap_or_else(PoisonError::into_inner) = mode;
    }

    /// The mode set, else off.
    pub fn mode(&self) -> Mode {
        *self.mode.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The lists of the names.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Whether a connection to `server_name` under `trust` is to be sealed.
    pub fn wanted(&self, server_name: &str, trust: &Trust) -> bool {
        asks(self.mode(), server_name, trust)
    }

    /// What ECH did for `name`'s last connection.
    pub fn outcome(&self, name: &str) -> Option<Outcome> {
        let name = key(name);
        self.kept().iter().find(|(n, _)| *n == name).map(|(_, outcome)| outcome.clone())
    }

    /// `name`'s last connection had `outcome`.
    fn note(&self, name: &str, outcome: Outcome) {
        let name = key(name);
        let mut kept = self.kept();
        kept.retain(|(n, _)| *n != name);
        if kept.len() >= OUTCOMES {
            kept.remove(0);
        }
        kept.push((name, outcome));
    }

    fn kept(&self) -> MutexGuard<'_, Vec<(String, Outcome)>> {
        self.outcomes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether `mode` seals a hello to `server_name` under `trust`: a name and
/// not an address, which rustls seals no hello for; of two labels at least,
/// as a host of a local network publishes no list; and the crate's own
/// trust, as a caller's configuration cannot be made again with a list.
fn asks(mode: Mode, server_name: &str, trust: &Trust) -> bool {
    let name = server_name.trim_end_matches('.');
    mode != Mode::Off
        && !matches!(trust, Trust::Caller(_))
        && name.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().is_err()
        && name.contains('.')
}

/// TCP to the target of `route` through its dialer, then TLS for its server
/// name with the roots of its trust, offering its ALPN: sealed where the
/// context asks for it and the name publishes a list, else a plain hello as
/// always; and the address that the TCP reached.
pub async fn open(context: &Context, route: &Route<'_>) -> Result<(TlsStream, Option<SocketAddr>), Failure> {
    let mode = context.ech.mode();
    if !asks(mode, route.server_name, route.trust) {
        return road::plain(context, route).await;
    }
    // On the heap: the sealed road's future (a read of the name's record,
    // then a sealed hello and its fallbacks) is large, and on the stack of
    // each dial, ECH asked or not, it overflowed a test's thread.
    Box::pin(road::sealed(context, &context.ech.cache, mode, route)).await
}

/// [`open`] in `mode` with the lists of `cache`, as a test holds them. The
/// outcome goes to the context's ECH, as [`open`]'s does.
pub async fn open_with(
    context: &Context,
    cache: &Cache,
    mode: Mode,
    route: &Route<'_>,
) -> Result<(TlsStream, Option<SocketAddr>), Failure> {
    if !asks(mode, route.server_name, route.trust) {
        return road::plain(context, route).await;
    }
    Box::pin(road::sealed(context, cache, mode, route)).await
}

/// A name as the state keeps it: lower case, with no final dot.
fn key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests;
