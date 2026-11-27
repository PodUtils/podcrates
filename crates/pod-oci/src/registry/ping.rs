//! `GET /v2/`: the check of a login against a registry, as `docker login`
//! does it, with no repository.

use super::wire::key;
use super::*;

/// How a registry took `GET /v2/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ping {
    /// The registry asked for a login, and took the stored one.
    Authenticated,
    /// The registry answered with no login: it asked for none, or a token
    /// realm gave a token with no login.
    Anonymous,
}

impl Client {
    /// `GET /v2/` at `endpoint` through the flow of each request: a `Basic`
    /// challenge gets the stored login, a `Bearer` challenge gets a token
    /// with no scope from its realm. A login that the registry or its realm
    /// refuses is `Err` of `Class::Refused`, with the reason of the registry.
    /// A registry that cannot be reached gives the class of the transport.
    ///
    /// Each ping asks again: it sends no token that the client keeps for
    /// the empty scope, so a changed login is checked, not a cached token.
    pub fn ping(&mut self, endpoint: &str) -> Result<Ping> {
        self.tokens.remove(&key(endpoint, ""));
        self.sent_login = false;
        let body = self.get("/v2/", endpoint, "", None)?;
        drop(body);
        Ok(if self.sent_login { Ping::Authenticated } else { Ping::Anonymous })
    }
}
