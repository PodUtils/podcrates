//! The commands of a policy session: `TPM2_StartAuthSession` (part 3,
//! clause 11.1), `TPM2_PolicyPCR` (23.7), `TPM2_PolicyAuthorize` (23.16)
//! and `TPM2_PolicyGetDigest` (23.19).

use super::object::Ticket;
use super::{CC_POLICY_AUTHORIZE, CC_POLICY_GET_DIGEST, CC_POLICY_PCR, CC_START_AUTH_SESSION};
use crate::alg::{ALG_NULL, HashAlg};
use crate::error::Error;
use crate::selection::PcrSelection;
use crate::transport::Reply;
use crate::wire::{MAX_DIGEST, Out, RH_NULL, command};

/// The fewest bytes of the nonce of a caller (part 3, 11.1.1: a shorter one
/// gives `TPM_RC_SIZE`).
pub const MIN_NONCE: usize = 16;

/// The kind of a session (`TPM_SE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Hmac,
    Policy,
    /// A policy session that computes the digest and checks nothing.
    Trial,
}

impl SessionKind {
    fn value(self) -> u8 {
        match self {
            SessionKind::Hmac => 0x00,
            SessionKind::Policy => 0x01,
            SessionKind::Trial => 0x03,
        }
    }
}

/// `TPM2_StartAuthSession` of `kind`, bound to nothing and salted with
/// nothing, with no symmetric algorithm and the hash `hash`. The caller
/// gives the nonce (from `pod_core::entropy::Entropy`): the crate reads no
/// random source. The nonce has from 16 bytes to the size of a digest of
/// `hash`.
pub fn start_auth_session(kind: SessionKind, nonce: &[u8], hash: HashAlg) -> Result<Vec<u8>, Error> {
    if !(MIN_NONCE..=hash.size()).contains(&nonce.len()) {
        return Err(Error::Usage(format!(
            "a nonce of {} bytes, where a session of {hash} takes {MIN_NONCE} to {}",
            nonce.len(),
            hash.size()
        )));
    }
    let mut params = Out::default();
    params.sized(nonce).sized(&[]).u8(kind.value()).u16(ALG_NULL).u16(hash.id());
    command(CC_START_AUTH_SESSION, &[RH_NULL, RH_NULL], &[], &params.finish()?)
}

/// What `TPM2_StartAuthSession` gives: the handle of the session and the
/// nonce of the TPM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub handle: u32,
    pub nonce_tpm: Vec<u8>,
}

impl Started {
    pub fn read(reply: &Reply) -> Result<Started, Error> {
        let handle = reply.handle()?;
        let mut p = reply.params();
        let nonce_tpm = p.sized_max(MAX_DIGEST, "a nonce")?.to_vec();
        p.finish("the response of TPM2_StartAuthSession")?;
        Ok(Started { handle, nonce_tpm })
    }
}

/// `TPM2_PolicyPCR` in `session`: `digest` of the values that the PCRs must
/// hold (the TPM compares it when it is not empty), and their selection.
pub fn policy_pcr(session: u32, digest: &[u8], selection: &PcrSelection) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.sized(digest).raw(&selection.marshal());
    command(CC_POLICY_PCR, &[session], &[], &params.finish()?)
}

/// `TPM2_PolicyAuthorize` in `session`: the approved policy and the policy
/// reference that the policy key signed, the name of the policy key, and
/// the ticket of `TPM2_VerifySignature` ([`Ticket::null`] in a trial
/// session).
pub fn policy_authorize(
    session: u32,
    approved: &[u8],
    policy_ref: &[u8],
    key_name: &[u8],
    ticket: &Ticket,
) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.sized(approved).sized(policy_ref).sized(key_name).raw(&ticket.0);
    command(CC_POLICY_AUTHORIZE, &[session], &[], &params.finish()?)
}

/// `TPM2_PolicyGetDigest`: the digest that `session` reached.
pub fn policy_get_digest(session: u32) -> Result<Vec<u8>, Error> {
    command(CC_POLICY_GET_DIGEST, &[session], &[], &[])
}

/// The digest of the response of `TPM2_PolicyGetDigest`.
pub fn policy_digest(reply: &Reply) -> Result<Vec<u8>, Error> {
    let mut p = reply.params();
    let digest = p.sized_max(MAX_DIGEST, "a policy digest")?.to_vec();
    p.finish("the response of TPM2_PolicyGetDigest")?;
    Ok(digest)
}
