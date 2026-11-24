//! The policy digest of a key, with no I/O. A policy is a list of policy
//! commands; the TPM extends the digest of a session with each, from a
//! digest of zeros, with the hash of the session (part 1, clause 19.7;
//! part 3, clause 23):
//!
//! * `TPM2_PolicyPCR` (23.7): `new = H(old || TPM_CC_PolicyPCR || pcrs ||
//!   digestTPM)`, where `digestTPM` is the hash of the values of the
//!   selected PCRs, in the order of the selection.
//! * `TPM2_PolicyAuthorize` (23.16, and `PolicyUpdate` of 23.2.3): the
//!   digest is set to zeros, then `H(H(zeros || TPM_CC_PolicyAuthorize ||
//!   keySign.name) || policyRef)`. The policy key signs `aHash =
//!   H(approvedPolicy || policyRef)` with its name algorithm.
//! * `TPM2_PolicyAuthValue` and `TPM2_PolicyPassword` (23.17, 23.18): `new
//!   = H(old || TPM_CC_PolicyAuthValue)`.
//!
//! A statement is a command code and a body, as a key file keeps it (the
//! draft of the key files, 4.1). The body of `TPM2_PolicyPCR` is the
//! selection, then the digest of the values (the layout that
//! openssl_tpm2_engine writes), so the bytes of the file are the bytes that
//! the digest hashes. The body of `TPM2_PolicyAuthorize` is the public area
//! of the policy key, the policy reference and the signature (4.2.1).

use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::alg::{ALG_ECDSA, ALG_NULL, ALG_RSASSA, HashAlg, describe};
use crate::cmd::{self, CC_POLICY_AUTH_VALUE, CC_POLICY_AUTHORIZE, CC_POLICY_PASSWORD, CC_POLICY_PCR};
use crate::error::Error;
use crate::selection::PcrSelection;
use crate::wire::{In, MAX_DIGEST, Out};

/// The hash of the concatenation of `parts` with `alg`. SHA-1 is refused:
/// the crate makes no digest of SHA-1, and it reads the size of a bank of
/// SHA-1 alone.
pub fn hash(alg: HashAlg, parts: &[&[u8]]) -> Result<Vec<u8>, Error> {
    fn of<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
        let mut d = D::new();
        for p in parts {
            d.update(p);
        }
        d.finalize().to_vec()
    }
    match alg {
        HashAlg::Sha256 => Ok(of::<Sha256>(parts)),
        HashAlg::Sha384 => Ok(of::<Sha384>(parts)),
        HashAlg::Sha512 => Ok(of::<Sha512>(parts)),
        HashAlg::Sha1 => {
            Err(Error::Unsupported("a digest of SHA-1: the crate hashes with SHA-256, SHA-384 and SHA-512".into()))
        }
    }
}

/// The digest of the values of PCRs with the hash of the session, in the
/// order of the selection: `digestTPM` of `TPM2_PolicyPCR`.
pub fn pcr_values_digest(alg: HashAlg, values: &[&[u8]]) -> Result<Vec<u8>, Error> {
    hash(alg, values)
}

fn check_previous(alg: HashAlg, previous: &[u8]) -> Result<(), Error> {
    if previous.len() != alg.size() {
        return Err(Error::Usage(format!(
            "a policy digest of {} bytes, where {alg} gives {}",
            previous.len(),
            alg.size()
        )));
    }
    Ok(())
}

/// The digest after `TPM2_PolicyPCR` from `previous` (the zeros of a new
/// session), with `selection` and the values of its PCRs in its order,
/// each of the size of its bank. A list of values that does not match the
/// selection is refused.
pub fn policy_pcr_digest(
    alg: HashAlg,
    previous: &[u8],
    selection: &PcrSelection,
    pcr_values: &[&[u8]],
) -> Result<Vec<u8>, Error> {
    selection.check_policy()?;
    if pcr_values.len() != selection.count() {
        return Err(Error::Usage(format!(
            "{} values for a selection of {} PCRs ({selection})",
            pcr_values.len(),
            selection.count()
        )));
    }
    let mut values = pcr_values.iter();
    for bank in selection.banks() {
        // check_policy found a known hash in each bank.
        let size = bank.hash().map_or(0, HashAlg::size);
        for pcr in bank.pcrs() {
            let value = values.next().copied().unwrap_or_default();
            if value.len() != size {
                return Err(Error::Usage(format!(
                    "the value of PCR {pcr} has {} bytes, where its bank has {size}",
                    value.len()
                )));
            }
        }
    }
    policy_pcr_update(alg, previous, selection, &pcr_values_digest(alg, pcr_values)?)
}

/// The digest after `TPM2_PolicyPCR` from `previous`, with `selection` and
/// the digest of the values of its PCRs (`digestTPM`, as a key file keeps
/// it).
pub fn policy_pcr_update(
    alg: HashAlg,
    previous: &[u8],
    selection: &PcrSelection,
    pcr_digest: &[u8],
) -> Result<Vec<u8>, Error> {
    check_previous(alg, previous)?;
    selection.check_policy()?;
    update_pcr(alg, previous, &selection.marshal(), pcr_digest)
}

fn update_pcr(alg: HashAlg, previous: &[u8], selection: &[u8], pcr_digest: &[u8]) -> Result<Vec<u8>, Error> {
    if pcr_digest.len() != alg.size() {
        return Err(Error::Usage(format!(
            "a digest of PCR values of {} bytes, where a session of {alg} needs {}",
            pcr_digest.len(),
            alg.size()
        )));
    }
    hash(alg, &[previous, &CC_POLICY_PCR.to_be_bytes(), selection, pcr_digest])
}

/// The bytes of a name: the algorithm, then a digest of its size.
fn check_name(name: &[u8]) -> Result<(), Error> {
    let alg = match name {
        [a, b, rest @ ..] => HashAlg::from_id(u16::from_be_bytes([*a, *b])).filter(|h| h.size() == rest.len()),
        _ => None,
    };
    alg.map(drop).ok_or_else(|| Error::Usage("a name that is not an algorithm and a digest of its size".into()))
}

fn check_ref(policy_ref: &[u8]) -> Result<(), Error> {
    if policy_ref.len() > MAX_DIGEST {
        return Err(Error::Usage(format!(
            "a policyRef of {} bytes, where a nonce has at most {MAX_DIGEST}",
            policy_ref.len()
        )));
    }
    Ok(())
}

/// The digest after `TPM2_PolicyAuthorize` of the policy key of the name
/// `key_name`, with `policy_ref`: it starts again from zeros, takes the
/// name, then the policy reference.
pub fn policy_authorize_digest(alg: HashAlg, key_name: &[u8], policy_ref: &[u8]) -> Result<Vec<u8>, Error> {
    check_name(key_name)?;
    check_ref(policy_ref)?;
    let zeros = vec![0u8; alg.size()];
    let first = hash(alg, &[&zeros, &CC_POLICY_AUTHORIZE.to_be_bytes(), key_name])?;
    hash(alg, &[&first, policy_ref])
}

/// The digest that the policy key signs (`aHash`): `H(approvedPolicy ||
/// policyRef)`, with the name algorithm of the policy key.
pub fn approved_policy_hash(alg: HashAlg, approved_policy: &[u8], policy_ref: &[u8]) -> Result<Vec<u8>, Error> {
    if approved_policy.len() > MAX_DIGEST {
        return Err(Error::Usage(format!("an approved policy of {} bytes", approved_policy.len())));
    }
    check_ref(policy_ref)?;
    hash(alg, &[approved_policy, policy_ref])
}

/// The name of a key from its public area (`TPM2B_PUBLIC`, whole): its
/// name algorithm, then the hash of the `TPMT_PUBLIC` with it (part 1,
/// clause 16).
pub fn key_name(public: &[u8]) -> Result<Vec<u8>, Error> {
    let mut input = In::new(public);
    let area = input.sized()?;
    input.finish("the public area")?;
    let id = match area {
        [_, _, a, b, ..] => u16::from_be_bytes([*a, *b]),
        _ => return Err(Error::Malformed("a public area too short for its name algorithm".into())),
    };
    let alg = HashAlg::from_id(id).filter(|a| *a != HashAlg::Sha1);
    let alg = alg.ok_or_else(|| Error::Unsupported(format!("a key named with {}", describe(id))))?;
    let mut name = id.to_be_bytes().to_vec();
    name.extend_from_slice(&hash(alg, &[area])?);
    Ok(name)
}

/// A `TPM2_PolicyAuthorize` as a key file keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorize {
    /// The public area of the policy key (`TPM2B_PUBLIC`, whole).
    pub public: Vec<u8>,
    pub policy_ref: Vec<u8>,
    /// The `TPMT_SIGNATURE`, whole: `public::NULL_SIGNATURE` in the policy
    /// of the key itself.
    pub signature: Vec<u8>,
}

impl Authorize {
    pub fn parse(body: &[u8]) -> Result<Authorize, Error> {
        let mut input = In::new(body);
        let public = input.sized_whole()?.to_vec();
        let policy_ref = input.sized_max(MAX_DIGEST, "a policyRef")?.to_vec();
        let signature = input.rest().to_vec();
        check_signature(&signature)?;
        Ok(Authorize { public, policy_ref, signature })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut out = Out::default();
        out.raw(&self.public).sized(&self.policy_ref).raw(&self.signature);
        out.finish()
    }
}

/// A `TPMT_SIGNATURE` that the crate passes to the TPM: NULL, ECDSA or
/// RSASSA, with nothing after it.
fn check_signature(signature: &[u8]) -> Result<(), Error> {
    let mut input = In::new(signature);
    match input.u16()? {
        ALG_NULL => {}
        ALG_ECDSA => {
            input.u16()?;
            input.sized()?;
            input.sized()?;
        }
        ALG_RSASSA => {
            input.u16()?;
            input.sized()?;
        }
        other => return Err(Error::Unsupported(format!("a signature of {} in a policy", describe(other)))),
    }
    input.finish("the signature of a policy")
}

/// One policy command: its code and its body, as a key file keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub code: u32,
    pub body: Vec<u8>,
}

impl Statement {
    /// `TPM2_PolicyPCR` of `selection` and the digest of its values.
    pub fn pcr(selection: &PcrSelection, pcr_digest: &[u8]) -> Statement {
        Statement { code: CC_POLICY_PCR, body: [selection.marshal().as_slice(), pcr_digest].concat() }
    }

    /// `TPM2_PolicyAuthorize` of `authorize`.
    pub fn authorize(authorize: &Authorize) -> Result<Statement, Error> {
        Ok(Statement { code: CC_POLICY_AUTHORIZE, body: authorize.to_bytes()? })
    }

    /// The selection of a `TPM2_PolicyPCR` and the digest of its values.
    /// The body is read in the layout of a key file, then in the order of
    /// the command (the digest as a `TPM2B` first), which other writers use.
    pub fn read_pcr(&self) -> Result<(PcrSelection, Vec<u8>), Error> {
        let (selection, digest) = split_pcr(&self.body)?;
        Ok((PcrSelection::from_bytes(selection)?, digest.to_vec()))
    }
}

const DIGEST_SIZES: [usize; 3] = [32, 48, 64];

/// The bytes of the selection and of the digest of a body of
/// `TPM2_PolicyPCR`.
fn split_pcr(body: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    let mut input = In::new(body);
    if PcrSelection::read(&mut input).is_ok() && DIGEST_SIZES.contains(&input.remaining()) {
        return Ok(body.split_at(body.len() - input.remaining()));
    }
    let mut input = In::new(body);
    let digest = input.sized()?;
    let selection = input.rest();
    PcrSelection::read(&mut input)?;
    if !input.is_empty() || !(digest.is_empty() || DIGEST_SIZES.contains(&digest.len())) {
        return Err(Error::Malformed("a body of TPM2_PolicyPCR in a layout that the crate does not know".into()));
    }
    Ok((selection, digest))
}

/// A policy: a list of statements.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    pub statements: Vec<Statement>,
}

impl Policy {
    pub fn new(statements: Vec<Statement>) -> Policy {
        Policy { statements }
    }

    /// The digest that the statements reach with the hash of the session
    /// `alg`: the `authPolicy` of a key. `TPM2_PolicyAuthorize` comes last.
    /// Another command than PolicyPCR, PolicyAuthorize, PolicyAuthValue and
    /// PolicyPassword is refused by its name.
    pub fn digest(&self, alg: HashAlg) -> Result<Vec<u8>, Error> {
        let mut current = vec![0u8; alg.size()];
        for (i, statement) in self.statements.iter().enumerate() {
            current = match statement.code {
                CC_POLICY_AUTHORIZE if i + 1 != self.statements.len() => {
                    return Err(Error::Usage("TPM2_PolicyAuthorize comes last in a policy".into()));
                }
                CC_POLICY_AUTHORIZE => {
                    let authorize = Authorize::parse(&statement.body)?;
                    policy_authorize_digest(alg, &key_name(&authorize.public)?, &authorize.policy_ref)?
                }
                CC_POLICY_PCR => {
                    let (selection, digest) = split_pcr(&statement.body)?;
                    // An empty digest means "the values of the day": no file
                    // can say what its policy digest is then.
                    PcrSelection::from_bytes(selection)?.check_policy()?;
                    update_pcr(alg, &current, selection, digest)?
                }
                CC_POLICY_AUTH_VALUE | CC_POLICY_PASSWORD if statement.body.is_empty() => {
                    // TPM2_PolicyPassword extends with the code of PolicyAuthValue.
                    hash(alg, &[&current, &CC_POLICY_AUTH_VALUE.to_be_bytes()])?
                }
                other => {
                    return Err(Error::Unsupported(format!(
                        "{} in a policy, which the crate does not compute",
                        cmd::name(other)
                    )));
                }
            };
        }
        Ok(current)
    }

    /// The approved policy of a signed policy: the digest of the statements
    /// before its last `TPM2_PolicyAuthorize`, which the policy key signed.
    pub fn approved(&self, alg: HashAlg) -> Result<Vec<u8>, Error> {
        match self.statements.split_last() {
            Some((last, before)) if last.code == CC_POLICY_AUTHORIZE => Policy::new(before.to_vec()).digest(alg),
            _ => Err(Error::Usage("the policy does not end with TPM2_PolicyAuthorize".into())),
        }
    }
}
