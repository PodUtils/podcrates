//! The commands of objects: `TPM2_CreatePrimary` (part 3, clause 24.1),
//! `TPM2_Create` (12.1), `TPM2_Load` (12.2), `TPM2_LoadExternal` (12.3),
//! `TPM2_Sign` (20.2), `TPM2_VerifySignature` (20.1) and
//! `TPM2_FlushContext` (28.4).

use super::{CC_CREATE, CC_CREATE_PRIMARY, CC_FLUSH_CONTEXT, CC_LOAD, CC_LOAD_EXTERNAL, CC_SIGN, CC_VERIFY_SIGNATURE};
use crate::alg::{ALG_ECDSA, ALG_SHA256};
use crate::error::Error;
use crate::public::EcdsaSignature;
use crate::transport::Reply;
use crate::wire::{Auth, In, MAX_DIGEST, MAX_NAME, Out, RH_NULL, command};

/// `TPM_ST_CREATION`: the tag of the ticket of a creation.
const ST_CREATION: u16 = 0x8021;
/// `TPM_ST_VERIFIED`: the tag of the ticket of a checked signature.
pub const ST_VERIFIED: u16 = 0x8022;
/// `TPM_ST_HASHCHECK`: the tag of the ticket of a digest that the TPM
/// hashed; a NULL one for a key that is not restricted.
const ST_HASHCHECK: u16 = 0x8024;

/// The fields of a creation that the crate leaves empty: no outside data
/// and no PCRs in the creation data.
fn creation_tail(out: &mut Out) {
    out.sized(&[]).u32(0);
}

/// The sensitive part of a new key (`TPM2B_SENSITIVE_CREATE`): no
/// authorization value and no data.
fn empty_sensitive(out: &mut Out) {
    out.u16(4).u16(0).u16(0);
}

/// `TPM2_CreatePrimary` under `hierarchy`, with its empty password, of the
/// `TPM2B_PUBLIC` `template` (whole).
pub fn create_primary(hierarchy: u32, template: &[u8]) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    empty_sensitive(&mut params);
    params.raw(template);
    creation_tail(&mut params);
    command(CC_CREATE_PRIMARY, &[hierarchy], &[Auth::EMPTY_PASSWORD], &params.finish()?)
}

/// `TPM2_Create` of the `TPM2B_PUBLIC` `template` (whole) under `parent`,
/// with the empty password of the parent.
pub fn create(parent: u32, template: &[u8]) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    empty_sensitive(&mut params);
    params.raw(template);
    creation_tail(&mut params);
    command(CC_CREATE, &[parent], &[Auth::EMPTY_PASSWORD], &params.finish()?)
}

/// `TPM2_Load` of the parts of a key (each a `TPM2B`, whole, as a key file
/// keeps it) under `parent`.
pub fn load(parent: u32, private: &[u8], public: &[u8]) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.raw(private).raw(public);
    command(CC_LOAD, &[parent], &[Auth::EMPTY_PASSWORD], &params.finish()?)
}

/// `TPM2_LoadExternal` of a public area alone (`TPM2B_PUBLIC`, whole) in
/// `hierarchy`. Part 3 (12.3.1) gives a NULL ticket for a key of the NULL
/// hierarchy, and `TPM2_PolicyAuthorize` refuses a NULL ticket: give the
/// owner hierarchy for the key of a policy.
pub fn load_external(public: &[u8], hierarchy: u32) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.u16(0).raw(public).u32(hierarchy);
    command(CC_LOAD_EXTERNAL, &[], &[], &params.finish()?)
}

/// `TPM2_VerifySignature` of `digest` by the loaded key `key`, with the
/// `TPMT_SIGNATURE` `signature`; its response is the [`Ticket`].
pub fn verify_signature(key: u32, digest: &[u8], signature: &[u8]) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.sized(digest).raw(signature);
    command(CC_VERIFY_SIGNATURE, &[key], &[], &params.finish()?)
}

/// `TPM2_Sign` of a SHA-256 `digest` with ECDSA by the key `key`, under the
/// authorization `auth`: a policy session ([`Auth::policy`]), or the empty
/// password, which a key under a policy refuses.
pub fn sign(key: u32, auth: &Auth, digest: &[u8; 32]) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.sized(digest).u16(ALG_ECDSA).u16(ALG_SHA256);
    params.u16(ST_HASHCHECK).u32(RH_NULL).sized(&[]);
    command(CC_SIGN, &[key], &[*auth], &params.finish()?)
}

/// `TPM2_FlushContext` of a handle: a key or a session.
pub fn flush_context(handle: u32) -> Result<Vec<u8>, Error> {
    command(CC_FLUSH_CONTEXT, &[], &[], &handle.to_be_bytes())
}

/// The ticket of a creation (`TPMT_TK_CREATION`), checked and passed over.
fn skip_creation_ticket(input: &mut In) -> Result<(), Error> {
    let tag = input.u16()?;
    if tag != ST_CREATION {
        return Err(Error::Malformed(format!("a creation ticket of tag {tag:#06x}")));
    }
    input.u32()?;
    input.sized_max(MAX_DIGEST, "the digest of a ticket")?;
    Ok(())
}

/// What `TPM2_CreatePrimary` gives: the handle of the primary, its public
/// area (`TPM2B_PUBLIC`, whole) and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primary {
    pub handle: u32,
    pub public: Vec<u8>,
    pub name: Vec<u8>,
}

impl Primary {
    pub fn read(reply: &Reply) -> Result<Primary, Error> {
        let handle = reply.handle()?;
        let mut p = reply.params();
        let public = p.sized_whole()?.to_vec();
        p.sized()?; // creationData
        p.sized_max(MAX_DIGEST, "the creation hash")?;
        skip_creation_ticket(&mut p)?;
        let name = p.sized_max(MAX_NAME, "a name")?.to_vec();
        p.finish("the response of TPM2_CreatePrimary")?;
        Ok(Primary { handle, public, name })
    }
}

/// What `TPM2_Create` gives: the private and the public parts, each a
/// `TPM2B`, whole, as a key file keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

impl Created {
    pub fn read(reply: &Reply) -> Result<Created, Error> {
        let mut p = reply.params();
        let private = p.sized_whole()?.to_vec();
        let public = p.sized_whole()?.to_vec();
        p.sized()?; // creationData
        p.sized_max(MAX_DIGEST, "the creation hash")?;
        skip_creation_ticket(&mut p)?;
        p.finish("the response of TPM2_Create")?;
        Ok(Created { private, public })
    }
}

/// What `TPM2_Load` and `TPM2_LoadExternal` give: the handle of the object
/// and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub handle: u32,
    pub name: Vec<u8>,
}

impl Loaded {
    pub fn read(reply: &Reply) -> Result<Loaded, Error> {
        let handle = reply.handle()?;
        let mut p = reply.params();
        let name = p.sized_max(MAX_NAME, "a name")?.to_vec();
        p.finish("the response of a load")?;
        Ok(Loaded { handle, name })
    }
}

/// The ticket of a checked signature (`TPMT_TK_VERIFIED`), whole, as
/// `TPM2_PolicyAuthorize` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket(pub Vec<u8>);

impl Ticket {
    /// The ticket of `TPM2_VerifySignature`.
    pub fn read(reply: &Reply) -> Result<Ticket, Error> {
        let mut p = reply.params();
        let start = p.rest();
        let tag = p.u16()?;
        if tag != ST_VERIFIED {
            return Err(Error::Malformed(format!("a ticket of tag {tag:#06x}")));
        }
        p.u32()?;
        p.sized_max(MAX_DIGEST, "the digest of a ticket")?;
        let whole = start[..start.len() - p.remaining()].to_vec();
        p.finish("the response of TPM2_VerifySignature")?;
        Ok(Ticket(whole))
    }

    /// The ticket of nothing, which a trial session takes.
    pub fn null() -> Ticket {
        let mut out = Vec::with_capacity(8);
        out.extend_from_slice(&ST_VERIFIED.to_be_bytes());
        out.extend_from_slice(&RH_NULL.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        Ticket(out)
    }
}

impl EcdsaSignature {
    /// The signature of `TPM2_Sign`.
    pub fn read(reply: &Reply) -> Result<EcdsaSignature, Error> {
        let mut p = reply.params();
        let signature = EcdsaSignature::read_from(&mut p)?;
        p.finish("the response of TPM2_Sign")?;
        Ok(signature)
    }
}
