//! Getting a relay token: the one that the user gave (`Settings::token`),
//! else the run's, else a cached one, else a fresh one minted with
//! `POST /v1/mint`
//! (self-service, no account; signed by this client's key where the relay
//! asks, podssh T-448).
//! Tokens go into the `X-Relay-Token` header and nowhere else: not into
//! output, errors, URLs or argv.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pod_ws::{ConnectError, Trust};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::cache::{self as token_cache, Chosen, MIN_REMAINING_MS};
use crate::context::Context;
use crate::relay::{Relay, default_relay_host, parse_relay};

/// Where a token came from. A rejected cached token is replaced once by a
/// fresh one; a rejected token that the user gave is reported instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Environment,
    Cache,
    Minted,
}

/// A relay token in memory, wiped on drop. Its `Debug` never shows it.
pub struct Token {
    secret: Zeroizing<String>,
    pub origin: Origin,
    /// Set when a minted token could not be cached (a token is then minted
    /// on every run), so the caller can say so once.
    pub cache_warning: Option<String>,
    /// Set when the token was cached, but not where the variable of the
    /// cache said, so the caller can say so once.
    pub cache_note: Option<String>,
    /// The relay that minted it (`host` or `host:port`), when known.
    pub minted_at: Option<String>,
}

impl Token {
    /// The token, for the request header and nothing else.
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("secret", &"<redacted>")
            .field("origin", &self.origin)
            .field("minted_at", &self.minted_at)
            .finish()
    }
}

/// Why no token could be had.
#[derive(Debug)]
pub enum TokenError {
    /// The token that the user gave is not a token.
    BadEnvironment,
    /// The mint endpoint could not be reached.
    Connect(ConnectError),
    /// `429`: the relay is limiting how fast tokens are minted.
    RateLimited { retry_after: Option<String> },
    /// `503`: this relay does not issue tokens.
    NotIssued,
    /// Any other answer.
    Failed { status: u16, detail: String },
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::BadEnvironment => write!(f, "the relay token that was given does not look like a relay token"),
            TokenError::Connect(e) => write!(f, "could not mint a relay token: {e}"),
            TokenError::RateLimited { retry_after: Some(s) } => {
                write!(f, "the relay is rate-limiting token minting; retry in {s} s")
            }
            TokenError::RateLimited { retry_after: None } => {
                write!(f, "the relay is rate-limiting token minting; retry shortly")
            }
            TokenError::NotIssued => {
                write!(
                    f,
                    "this relay does not issue tokens (HTTP 503); ask its operator for one and give it as the relay token"
                )
            }
            TokenError::Failed { status, detail } => write!(f, "minting a relay token failed: HTTP {status}{detail}"),
        }
    }
}

/// What minting needs: the relay host to mint at (and to send the token to),
/// and how to reach it.
pub struct MintContext<'a> {
    /// The way out, the cache and the settings.
    pub ctx: &'a Context,
    pub relay: &'a Relay,
    pub trust: &'a Trust,
    pub timeout: Duration,
}

/// The name a token is cached under: one for each relay deployment. The
/// hosts of the default deployment share the default host's token (a pool
/// host accepts it, as measured); any other host has its own, with
/// its port when that is not 443. So a token never goes to a deployment that
/// did not mint it (podssh's operator).
pub fn token_key(relay: &Relay) -> String {
    if relay.host == default_relay_host() || crate::pool::same_deployment(default_relay_host(), &relay.host) {
        default_relay_host().to_string()
    } else {
        relay_name(relay)
    }
}

/// `host`, or `host:port` when the port is not 443.
pub fn relay_name(relay: &Relay) -> String {
    if relay.port == 443 { relay.host.clone() } else { format!("{}:{}", relay.host, relay.port) }
}

/// Whether a cached token may be sent under `key`: only when its entry names
/// the relay that minted it, and that relay has the same key. An entry with
/// no minting relay was cached under the first host of a list, also when
/// another host minted it, so it is not used: one more mint costs less than
/// a token sent to the wrong deployment.
pub fn usable(key: &str, minted_at: Option<&str>) -> bool {
    minted_at.and_then(|m| parse_relay(m).ok()).is_some_and(|r| token_key(&r) == key)
}

/// The tokens of a run, by the key of their deployment ([`token_key`]): the
/// sessions of a run take the token that one of them minted or read, also
/// where the cache keeps no file, so that a long run mints once for each
/// token's life and not for each session (podnet). A refused token is
/// forgotten ([`forget`]).
#[derive(Default)]
pub struct RunTokens(Mutex<HashMap<String, Kept>>);

/// A token of the run, wiped on drop.
struct Kept {
    secret: Zeroizing<String>,
    expires_ms: i64,
    minted_at: Option<String>,
}

impl RunTokens {
    /// The token of `key` with at least `MIN_REMAINING_MS` left at `now_ms`.
    fn recall(&self, key: &str, now_ms: i64) -> Option<(Zeroizing<String>, Option<String>)> {
        let kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let token = kept.get(key).filter(|t| t.expires_ms.saturating_sub(now_ms) >= MIN_REMAINING_MS)?;
        Some((token.secret.clone(), token.minted_at.clone()))
    }

    fn keep(&self, key: &str, secret: &str, expires_ms: i64, minted_at: Option<String>) {
        let token = Kept { secret: Zeroizing::new(secret.to_string()), expires_ms, minted_at };
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), token);
    }

    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

/// Keep `token` as the run's token of `key` until `expires_ms`: the next
/// sessions of the run take it.
pub fn remember(ctx: &Context, key: &str, token: &str, expires_ms: i64, minted_at: Option<&str>) {
    ctx.run_tokens().keep(key, token, expires_ms, minted_at.map(str::to_string));
}

/// Forget the token of `key`, the run's and the cache's: the relay refused
/// it, so no session reads it again.
pub fn forget(ctx: &Context, key: &str) {
    ctx.run_tokens().forget(key);
    ctx.cache.remove(key);
}

/// The token to use. With `fresh`, skip the given token, the run's and the
/// cache (after the relay rejected a cached token).
pub async fn obtain(ctx: &MintContext<'_>, fresh: bool) -> Result<Token, TokenError> {
    let key = token_key(ctx.relay);
    let cache = &ctx.ctx.cache;
    if !fresh {
        let given = ctx.ctx.settings.token.as_deref().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(value) = given {
            if !token_cache::valid_token(&value) {
                return Err(TokenError::BadEnvironment);
            }
            let secret = Zeroizing::new(value);
            return Ok(Token {
                secret,
                origin: Origin::Environment,
                cache_warning: None,
                cache_note: None,
                minted_at: None,
            });
        }
        if let Some((secret, minted_at)) = ctx.ctx.run_tokens().recall(&key, now_ms()) {
            return Ok(Token { secret, origin: Origin::Cache, cache_warning: None, cache_note: None, minted_at });
        }
        if let Some(cached) = cache.load(&key, now_ms()).filter(|c| usable(&key, c.minted_at.as_deref())) {
            remember(ctx.ctx, &key, &cached.token, cached.expires_ms, cached.minted_at.as_deref());
            let secret = Zeroizing::new(cached.token);
            return Ok(Token {
                secret,
                origin: Origin::Cache,
                cache_warning: None,
                cache_note: None,
                minted_at: cached.minted_at,
            });
        }
    }
    let (token, expires) = mint(ctx).await?;
    let minted_at = relay_name(ctx.relay);
    remember(ctx.ctx, &key, &token, expires, Some(&minted_at));
    let (cache_warning, cache_note) = match cache.chosen {
        // The user keeps no file: a token is minted on each run, as asked.
        Chosen::Nothing => (None, None),
        _ => match cache.store_noted(&key, &token, expires, &minted_at) {
            Ok(stored) => (None, cache.chosen_note(&ctx.ctx.settings.names.cache_dir, &stored)),
            Err(why) => (Some(why), None),
        },
    };
    Ok(Token {
        secret: Zeroizing::new(token),
        origin: Origin::Minted,
        cache_warning,
        cache_note,
        minted_at: Some(minted_at),
    })
}

#[derive(Deserialize)]
struct MintResponse {
    token: String,
    /// Milliseconds since the epoch.
    expires: i64,
}

/// `POST /v1/mint` with `{}`, and once more with this client's key when the
/// relay asks for it; the relay answers `{token, expires, scope}`.
async fn mint(ctx: &MintContext<'_>) -> Result<(String, i64), TokenError> {
    let response = crate::keyed::post(ctx.ctx, &ctx.relay.host, ctx.relay.port, "/v1/mint", ctx.trust, ctx.timeout)
        .await
        .map_err(TokenError::Connect)?;
    match response.status {
        200..=299 => {
            // Never quote a 2xx body in an error: it may hold a token.
            let parsed: MintResponse = serde_json::from_slice(&response.body).map_err(|_| TokenError::Failed {
                status: response.status,
                detail: ": the response was not the expected JSON".into(),
            })?;
            if !token_cache::valid_token(&parsed.token) {
                return Err(TokenError::Failed {
                    status: response.status,
                    detail: ": the response did not contain a usable token".into(),
                });
            }
            Ok((parsed.token, parsed.expires))
        }
        429 => Err(TokenError::RateLimited { retry_after: response.header_text("retry-after", 64) }),
        503 => Err(TokenError::NotIssued),
        status => {
            let body = response.body_text(200);
            Err(TokenError::Failed {
                status,
                detail: if body.is_empty() { String::new() } else { format!(": {body}") },
            })
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
