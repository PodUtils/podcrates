//! A signature with a loaded key under its policy. The policy runs in a
//! fresh policy session, which `TPM2_Sign` then uses. A key under a policy
//! key takes each signed policy of its file in turn, the first first, until
//! the TPM takes one (the draft of the key files, 5.2): the statements of
//! the signed policy, then `TPM2_LoadExternal` of the policy key,
//! `TPM2_VerifySignature` of its signature and `TPM2_PolicyAuthorize` with
//! the ticket, then the rest of the key's own policy. Before `TPM2_Sign`,
//! `TPM2_PolicyGetDigest` must give the `authPolicy` of the key. The policy
//! key signs off the TPM, in the caller; the crate only checks and uses the
//! signatures.

use pod_core::Class;
use pod_core::entropy::Entropy;

use super::{Handle, Held, code_of, fail, nonce, session, step};
use crate::alg::HashAlg;
use crate::cmd::{self, CC_POLICY_AUTHORIZE, CC_POLICY_PCR, SessionKind, Ticket};
use crate::error::Error;
use crate::keyfile::{AuthPolicy, KeyFile, MAX_AUTH_POLICIES};
use crate::policy::{Authorize, Policy, Statement, approved_policy_hash, key_name};
use crate::public::{EccPublic, EcdsaSignature};
use crate::rc::Code;
use crate::transport::{Transport, transact};
use crate::wire::{Auth, RH_OWNER};

/// The words of a key whose signed policies are none of them met.
pub const NO_SIGNED_POLICY: &str = "no signed policy for this boot state";

/// Sign the SHA-256 `digest` with the loaded `key` of `file`. A refusal of
/// the policy (the PCRs changed, no signed policy is met) is of the class
/// `Policy`. Each session and each outside key is flushed on each path; a
/// session that signed is ended by the TPM itself (`continueSession`
/// clear).
pub fn sign(
    key: &mut Handle<'_>,
    entropy: &mut dyn Entropy,
    file: &KeyFile,
    digest: &[u8; 32],
) -> Result<EcdsaSignature, Error> {
    if !file.empty_auth {
        return Err(Error::Unsupported("the key needs an authorization value, which the crate does not give".into()));
    }
    let wanted = EccPublic::read(&file.public)?.auth_policy;
    let statements = &file.policy.statements;
    let Some(first) = statements.first() else {
        return Err(Error::Unsupported("the key file has no policy: the crate signs with keys under a policy".into()));
    };
    if first.code != CC_POLICY_AUTHORIZE {
        let key_handle = key.handle();
        let (mut session, _) = session(key.transport(), entropy, SessionKind::Policy)?;
        let handle = session.handle();
        for statement in statements {
            run_step(session.transport(), handle, statement)?;
        }
        return sign_in(session, entropy, key_handle, &wanted, digest);
    }
    let head = Authorize::parse(&first.body)?;
    if file.auth_policy.is_empty() {
        return Err(fail(Class::Policy, format!("{NO_SIGNED_POLICY}: the key file holds no signed policy")));
    }
    let mut unmet = Vec::with_capacity(file.auth_policy.len());
    for signed in &file.auth_policy {
        match attempt(key, entropy, &head, signed, &statements[1..], &wanted, digest) {
            Ok(signature) => return Ok(signature),
            Err(Attempt::Unmet(why)) => unmet.push(match &signed.name {
                Some(name) => format!("{name:?}: {why}"),
                None => why.to_string(),
            }),
            Err(Attempt::Stop(e)) => return Err(e),
        }
    }
    Err(fail(Class::Policy, format!("{NO_SIGNED_POLICY}: {}", unmet.join("; "))))
}

/// Why one signed policy gave no signature: the TPM refused it, so the
/// next one can work; or another failure, which ends the signature.
enum Attempt {
    Unmet(Error),
    Stop(Error),
}

/// A refusal of the class `Policy` (the PCRs, a signature, the policy)
/// lets the next signed policy run; another failure (a TPM in its failure
/// mode, a lost transport) ends the signature with its own class.
fn classify(e: Error) -> Attempt {
    if e.class() == Class::Policy { Attempt::Unmet(e) } else { Attempt::Stop(e) }
}

/// One signed policy in a fresh session, then the rest of the key's own
/// policy, then the signature.
fn attempt(
    key: &mut Handle<'_>,
    entropy: &mut dyn Entropy,
    head: &Authorize,
    signed: &AuthPolicy,
    rest: &[Statement],
    wanted: &[u8],
    digest: &[u8; 32],
) -> Result<EcdsaSignature, Attempt> {
    let unmet = |why: &str| Attempt::Unmet(fail(Class::Policy, why));
    let Some((last, steps)) = signed.policy.statements.split_last() else {
        return Err(unmet("a signed policy with no statement"));
    };
    let authorizing = Authorize::parse(&last.body).map_err(Attempt::Unmet)?;
    if authorizing.public != head.public || authorizing.policy_ref != head.policy_ref {
        return Err(unmet("a signed policy of another policy key, or of another policyRef"));
    }
    if steps.iter().any(|s| s.code == CC_POLICY_AUTHORIZE) {
        return Err(unmet("a signed policy with a second policy key, which the crate does not run"));
    }
    let approved = Policy::new(steps.to_vec()).digest(HashAlg::Sha256).map_err(Attempt::Unmet)?;
    let key_handle = key.handle();
    let (mut session, _) = session(key.transport(), entropy, SessionKind::Policy).map_err(Attempt::Stop)?;
    let handle = session.handle();
    for statement in steps {
        run_step(session.transport(), handle, statement).map_err(classify)?;
    }
    take_signed(session.transport(), handle, &authorizing, &approved).map_err(classify)?;
    for statement in rest {
        run_step(session.transport(), handle, statement).map_err(classify)?;
    }
    sign_in(session, entropy, key_handle, wanted, digest).map_err(classify)
}

/// One statement of a policy in `session`: PolicyPCR alone, as the crate
/// runs no other statement after a PolicyAuthorize.
fn run_step(transport: &mut dyn Transport, session: u32, statement: &Statement) -> Result<(), Error> {
    if statement.code != CC_POLICY_PCR {
        return Err(Error::Unsupported(format!(
            "{} in the policy of a key: the crate runs PolicyPCR and PolicyAuthorize",
            cmd::name(statement.code)
        )));
    }
    let (selection, values) = statement.read_pcr()?;
    let command = cmd::policy_pcr(session, &values, &selection)?;
    let reply = transact(transport, &command).map_err(|e| match code_of(&e) {
        Some(Code::Value) => Error::Step {
            class: Class::Policy,
            why: format!(
                "the PCRs {selection} hold other values than the policy names: the boot state changed, and the TPM refuses the key"
            ),
            cause: Some(Box::new(e)),
        },
        _ => step("the TPM did not run TPM2_PolicyPCR")(e),
    })?;
    reply.empty()
}

/// The TPM checks the signature of `signed` over the approved policy, and
/// its ticket lets `TPM2_PolicyAuthorize` put the key's digest in
/// `session`. The policy key is loaded in the owner hierarchy: a key of the
/// NULL hierarchy gives a NULL ticket (part 3, 12.3.1).
fn take_signed(transport: &mut dyn Transport, session: u32, signed: &Authorize, approved: &[u8]) -> Result<(), Error> {
    let name = key_name(&signed.public)?;
    let alg = HashAlg::from_id(u16::from_be_bytes([name[0], name[1]]))
        .ok_or_else(|| Error::Unsupported("the name algorithm of the policy key".into()))?;
    let a_hash = approved_policy_hash(alg, approved, &signed.policy_ref)?;
    let command = cmd::load_external(&signed.public, RH_OWNER)?;
    let reply = transact(transport, &command).map_err(step("the TPM did not load the policy key"))?;
    let mut external = Handle::new(transport, reply.handle()?, Held::External);
    if cmd::Loaded::read(&reply)?.name != name {
        return Err(fail(Class::Protocol, "the TPM names the policy key otherwise than the crate does"));
    }
    let command = cmd::verify_signature(external.handle(), &a_hash, &signed.signature)?;
    let reply = transact(external.transport(), &command).map_err(|e| match code_of(&e) {
        Some(Code::Signature) => {
            step("the signature of the signed policy is not the policy key's: the key file was changed")(e)
        }
        _ => step("the TPM did not check the signature of the signed policy")(e),
    })?;
    let ticket = Ticket::read(&reply)?;
    external.finish()?;
    let command = cmd::policy_authorize(session, approved, &signed.policy_ref, &name, &ticket)?;
    transact(transport, &command).map_err(step("the TPM did not take the signed policy"))?.empty()
}

/// `TPM2_PolicyGetDigest` must give `wanted`, then `TPM2_Sign` in the
/// session. A session that signed is ended by the TPM.
fn sign_in(
    mut session: Handle<'_>,
    entropy: &mut dyn Entropy,
    key: u32,
    wanted: &[u8],
    digest: &[u8; 32],
) -> Result<EcdsaSignature, Error> {
    let command = cmd::policy_get_digest(session.handle())?;
    let reply = transact(session.transport(), &command)?;
    if cmd::policy_digest(&reply)? != wanted {
        return Err(fail(Class::Policy, "the policy session reached another digest than the authPolicy of the key"));
    }
    let nonce = nonce(entropy)?;
    let command = cmd::sign(key, &Auth::policy(session.handle(), &nonce), digest)?;
    let reply = transact(session.transport(), &command).map_err(step("the TPM did not sign"))?;
    session.release();
    EcdsaSignature::read(&reply)
}

/// `file` with the signed policy `signed` before the others. Its last
/// statement must be a `TPM2_PolicyAuthorize` of the policy key and the
/// policyRef that the file's policy names; a signed policy of the same
/// statements (the same boot state) is replaced.
pub fn add_signed_policy(file: &KeyFile, signed: AuthPolicy) -> Result<KeyFile, Error> {
    let head = match file.policy.statements.first() {
        Some(first) if first.code == CC_POLICY_AUTHORIZE => Authorize::parse(&first.body)?,
        _ => {
            return Err(Error::Usage(
                "the key is under its PCRs alone, with no policy key: a new boot state needs a new key".into(),
            ));
        }
    };
    let last = match signed.policy.statements.last() {
        Some(last) if last.code == CC_POLICY_AUTHORIZE => Authorize::parse(&last.body)?,
        _ => return Err(Error::Usage("a signed policy ends with TPM2_PolicyAuthorize".into())),
    };
    if last.public != head.public || last.policy_ref != head.policy_ref {
        return Err(Error::Usage("the policy key or the policyRef is not the one that the key file names".into()));
    }
    let steps = |p: &AuthPolicy| p.policy.statements.split_last().map(|(_, before)| before.to_vec());
    let mut new = file.clone();
    new.auth_policy.retain(|p| steps(p) != steps(&signed));
    new.auth_policy.insert(0, signed);
    // The oldest signed policy goes when the file holds the most.
    new.auth_policy.truncate(MAX_AUTH_POLICIES);
    new.to_der()?;
    Ok(new)
}
