//! The sequences of commands for an ECDSA P-256 key under a policy: the
//! storage primary, a new signing key and its key file, the load of a key
//! file, a signature ([`sign`]), the PCRs and the capabilities ([`pcr`]).
//! Each runs over a [`Transport`], and each handle that the TPM gives is in
//! a [`Handle`] guard at once: it is flushed on each path, so a failure
//! leaves the TPM as it found it. A TPM holds few transient objects and
//! sessions; a handle that stays fills the table, and the next command
//! fails with a resource error.
//!
//! The nonces come from the [`Entropy`] of the caller. The crate has no
//! `Unseal` and no flow for sealed data (call C931).

pub mod pcr;
pub mod sign;

use pod_core::Class;
use pod_core::entropy::Entropy;

use crate::alg::HashAlg;
use crate::cmd::{self, CC_POLICY_AUTHORIZE, CC_POLICY_PCR, Created, Primary, SessionKind, Started, Ticket};
use crate::error::Error;
use crate::keyfile::{KeyFile, PERMANENT, PERSISTENT};
use crate::policy::{Authorize, Policy, key_name};
use crate::public::{self, EccPublic};
use crate::rc::Code;
use crate::transport::{Transport, transact};

/// The bytes of each nonce of the caller: the size of a digest of SHA-256.
pub const NONCE: usize = 32;

/// What a guarded handle holds, for the words of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// A primary key that `TPM2_CreatePrimary` made.
    Primary,
    /// A key that `TPM2_Load` loaded.
    Loaded,
    /// A policy session.
    Session,
    /// A public key that `TPM2_LoadExternal` loaded.
    External,
}

/// A handle of the TPM that is flushed with `TPM2_FlushContext` when the
/// guard drops. [`Handle::finish`] flushes and returns the error of the
/// flush; a failed flush on drop goes to `tracing`. The guard holds the
/// transport, so a command with the handle goes through
/// [`Handle::transport`].
pub struct Handle<'t> {
    transport: &'t mut dyn Transport,
    handle: u32,
    held: Held,
    live: bool,
}

impl<'t> Handle<'t> {
    /// Guard `handle`, which the TPM gave on `transport`.
    pub fn new(transport: &'t mut dyn Transport, handle: u32, held: Held) -> Handle<'t> {
        Handle { transport, handle, held, live: true }
    }

    pub fn handle(&self) -> u32 {
        self.handle
    }

    pub fn held(&self) -> Held {
        self.held
    }

    /// The transport, for a command with the handle or a nested guard.
    pub fn transport(&mut self) -> &mut dyn Transport {
        &mut *self.transport
    }

    /// Flush the handle now, and return the error of the flush.
    pub fn finish(mut self) -> Result<(), Error> {
        self.live = false;
        flush(&mut *self.transport, self.handle)
    }

    /// Give up the guard with no flush: the TPM ended the handle itself (a
    /// session after a command that succeeded with `continueSession`
    /// clear).
    pub fn release(mut self) -> u32 {
        self.live = false;
        self.handle
    }
}

impl Drop for Handle<'_> {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        if let Err(e) = flush(&mut *self.transport, self.handle) {
            tracing::warn!(handle = self.handle, held = ?self.held, error = %e, "the TPM did not flush a handle");
        }
    }
}

/// `TPM2_FlushContext` of `handle`.
pub fn flush(transport: &mut dyn Transport, handle: u32) -> Result<(), Error> {
    transact(transport, &cmd::flush_context(handle)?)?.empty()
}

/// The error of a step: `why`, with the class of the error under it.
pub(crate) fn step(why: impl Into<String>) -> impl FnOnce(Error) -> Error {
    move |cause| Error::Step { class: cause.class(), why: why.into(), cause: Some(Box::new(cause)) }
}

/// The error of a step of `class` with no command under it.
pub(crate) fn fail(class: Class, why: impl Into<String>) -> Error {
    Error::Step { class, why: why.into(), cause: None }
}

/// The base code of a refusal of the TPM, if the error is one or a step
/// over one.
pub(crate) fn code_of(e: &Error) -> Option<Code> {
    e.code().map(|code| code.code())
}

/// The storage primary of `hierarchy` (the TCG template H-2, P-256, as the
/// draft of the key files says): its guard and what the TPM gave. The guard
/// comes before the read of the response, so a response that cannot be
/// read still flushes the handle.
pub fn primary(transport: &mut dyn Transport, hierarchy: u32) -> Result<(Handle<'_>, Primary), Error> {
    let command = cmd::create_primary(hierarchy, &public::storage_primary()?)?;
    let reply = transact(transport, &command).map_err(|e| match code_of(&e) {
        Some(Code::BadAuth | Code::AuthFail) => {
            step(format!("the hierarchy {hierarchy:#010x} has a password, and the crate gives the empty one"))(e)
        }
        _ => step("the TPM did not make its storage primary")(e),
    })?;
    let guard = Handle::new(transport, reply.handle()?, Held::Primary);
    let primary = Primary::read(&reply)?;
    Ok((guard, primary))
}

/// A policy session (or a trial one) with the hash SHA-256 and a fresh
/// nonce: its guard and its nonce of the TPM.
pub fn session<'t>(
    transport: &'t mut dyn Transport,
    entropy: &mut dyn Entropy,
    kind: SessionKind,
) -> Result<(Handle<'t>, Started), Error> {
    let nonce = nonce(entropy)?;
    let command = cmd::start_auth_session(kind, &nonce, HashAlg::Sha256)?;
    let reply = transact(transport, &command).map_err(step("the TPM did not start a session"))?;
    let guard = Handle::new(transport, reply.handle()?, Held::Session);
    let started = Started::read(&reply)?;
    Ok((guard, started))
}

/// A nonce of [`NONCE`] bytes from the caller's entropy.
pub fn nonce(entropy: &mut dyn Entropy) -> Result<[u8; NONCE], Error> {
    let mut nonce = [0u8; NONCE];
    entropy.fill(&mut nonce).map_err(|e| fail(Class::Unavailable, e.to_string()))?;
    Ok(nonce)
}

/// Run `body` with the handle of the parent `parent`: a persistent handle
/// as it is, or the storage primary of a permanent one, which is flushed
/// after `body`. Another handle is refused.
fn with_parent<R>(
    transport: &mut dyn Transport,
    parent: u32,
    body: impl FnOnce(&mut dyn Transport, u32) -> Result<R, Error>,
) -> Result<R, Error> {
    match parent.to_be_bytes()[0] {
        PERSISTENT => body(transport, parent).map_err(|e| match code_of(&e) {
            Some(Code::Handle) => Error::Step {
                class: Class::Unavailable,
                why: format!("the TPM has no persistent key {parent:#010x}"),
                cause: Some(Box::new(e)),
            },
            _ => e,
        }),
        PERMANENT => {
            let (mut guard, primary) = primary(transport, parent)?;
            let result = body(guard.transport(), primary.handle);
            drop(guard);
            result
        }
        _ => Err(Error::Usage(format!("the parent {parent:#010x} is neither a permanent nor a persistent handle"))),
    }
}

/// The digest that the TPM reaches for `policy` in a trial session, which
/// checks nothing. `TPM2_PolicyAuthorize` takes the ticket of nothing.
pub fn trial_digest(
    transport: &mut dyn Transport,
    entropy: &mut dyn Entropy,
    policy: &Policy,
) -> Result<Vec<u8>, Error> {
    let (mut trial, _) = session(transport, entropy, SessionKind::Trial)?;
    for statement in &policy.statements {
        let handle = trial.handle();
        let command = match statement.code {
            CC_POLICY_PCR => {
                let (selection, digest) = statement.read_pcr()?;
                cmd::policy_pcr(handle, &digest, &selection)?
            }
            CC_POLICY_AUTHORIZE => {
                let authorize = Authorize::parse(&statement.body)?;
                let name = key_name(&authorize.public)?;
                cmd::policy_authorize(handle, &[], &authorize.policy_ref, &name, &Ticket::null())?
            }
            other => {
                return Err(Error::Unsupported(format!(
                    "{} in the policy of a new key: the crate runs PolicyPCR and PolicyAuthorize",
                    cmd::name(other)
                )));
            }
        };
        transact(trial.transport(), &command).map_err(step("the TPM's trial of the policy failed"))?.empty()?;
    }
    let command = cmd::policy_get_digest(trial.handle())?;
    let reply = transact(trial.transport(), &command)?;
    let digest = cmd::policy_digest(&reply)?;
    trial.finish()?;
    Ok(digest)
}

/// A new ECDSA P-256 signing key under `parent` (the owner's primary
/// `0x40000001`, or the persistent storage key `0x81000001`), whose
/// `authPolicy` is the digest of `policy`: its key file (`Loadable`, with
/// no authorization value) and its public area. The TPM's own digest of the
/// policy, in a trial session, must be the crate's, and the public area
/// that it made must carry it and the attributes asked; else no key file
/// comes.
pub fn create_signing(
    transport: &mut dyn Transport,
    entropy: &mut dyn Entropy,
    parent: u32,
    policy: &Policy,
) -> Result<(KeyFile, EccPublic), Error> {
    let wanted = policy.digest(HashAlg::Sha256)?;
    if trial_digest(transport, entropy, policy)? != wanted {
        return Err(fail(Class::Protocol, "the TPM's digest of the policy is not the crate's: no key is made"));
    }
    let template = public::signing_key(&wanted)?;
    let created = with_parent(transport, parent, |t, handle| {
        let reply = transact(t, &cmd::create(handle, &template)?).map_err(step("the TPM did not make the key"))?;
        Created::read(&reply)
    })?;
    let area = EccPublic::read(&created.public)?;
    if area.auth_policy != wanted || area.attributes != public::SIGNING {
        return Err(fail(Class::Protocol, "the TPM made a key of another policy or other attributes than asked"));
    }
    let mut file = KeyFile::loadable(parent, created.public, created.private);
    file.policy = policy.clone();
    Ok((file, area))
}

/// Load the key of `file` under its parent: a persistent parent as it is,
/// a permanent one through its storage primary, which is flushed after the
/// load. A file of an RSA parent is refused.
pub fn load<'t>(transport: &'t mut dyn Transport, file: &KeyFile) -> Result<Handle<'t>, Error> {
    if file.rsa_parent {
        return Err(Error::Unsupported(
            "the key file names an RSA primary (rsaParent): the crate makes the P-256 primary (H-2) only".into(),
        ));
    }
    let key = with_parent(transport, file.parent, |t, parent| {
        let command = cmd::load(parent, &file.private, &file.public)?;
        let reply = transact(t, &command).map_err(|e| match code_of(&e) {
            Some(Code::Integrity) => step(
                "this TPM cannot load the key: another TPM made it, or the seed of its owner changed (a clear of the TPM)",
            )(e),
            _ => step("the TPM did not load the key")(e),
        })?;
        let guard = Handle::new(t, reply.handle()?, Held::Loaded);
        cmd::Loaded::read(&reply)?;
        // The guard of the parent's primary borrows the transport: the
        // handle of the key goes out of this guard and into a new one, with
        // no command between them.
        Ok(guard.release())
    })?;
    Ok(Handle::new(transport, key, Held::Loaded))
}

/// The point of the public key of `file`, uncompressed (0x04, X, Y): a
/// caller makes its own wire form of it. A key of another type or curve is
/// refused.
pub fn public_point(file: &KeyFile) -> Result<[u8; 65], Error> {
    Ok(EccPublic::read(&file.public)?.point)
}
