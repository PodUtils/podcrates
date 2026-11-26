//! A node's check of an operator's credential used in the place of its
//! admission: a grant or a bearer, each with its own words ([`crate::ends`]
//! re-exports this module's items).

use pod_identity::PublicKey;

use crate::Ask;

/// The form of an operator's credential (`docs/wire.md`, section 6): a
/// node says which form it let in or refused, to the operator and in its
/// log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// An OpenSSH certificate of an issuer, used once.
    Grant,
    /// A token of a key of the user, of 30 s, for one channel alone.
    Bearer,
}

impl Credential {
    /// The words of the credential in a node's line: "a grant" or "a bearer".
    pub fn words(self) -> &'static str {
        match self {
            Credential::Grant => "a grant",
            Credential::Bearer => "a bearer",
        }
    }

    /// What the operator hears when the node refuses this credential:
    /// [`GRANT_REFUSED`] or [`BEARER_REFUSED`].
    pub fn refused(self) -> &'static str {
        match self {
            Credential::Grant => GRANT_REFUSED,
            Credential::Bearer => BEARER_REFUSED,
        }
    }
}

/// What a node's [`Redeem`] found: the form of the credential, and `Ok`
/// with the words of the node's event, or `Err` with why not, for the
/// node's event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redemption {
    pub credential: Credential,
    pub result: Result<String, String>,
}

impl Redemption {
    /// The check of a grant.
    pub fn grant(result: Result<String, String>) -> Redemption {
        Redemption { credential: Credential::Grant, result }
    }

    /// The check of a bearer.
    pub fn bearer(result: Result<String, String>) -> Redemption {
        Redemption { credential: Credential::Bearer, result }
    }
}

/// A node's check of an operator's credential: the operator's key, which
/// the channel proved, what it asked, and the credential's bytes. A result
/// of `Ok` lets it in, once.
pub type Redeem = dyn Fn(&PublicKey, &Ask, &[u8]) -> Redemption + Send + Sync;

/// What an operator whose grant the node refused hears: the reasons stay in
/// the node's events, as they can name its files.
pub const GRANT_REFUSED: &str = "this grant does not let this key in: it was used already, its window is not now, or \
                                 it is not for this key, this node or this route; the node's log says which";

/// What an operator whose bearer the node refused hears, as for
/// [`GRANT_REFUSED`].
pub const BEARER_REFUSED: &str = "this bearer does not let this key in: it was used already, its 30 s passed, the \
                                  node does not take its signer for this, or it is not for this key or this \
                                  channel; the node's log says which";
