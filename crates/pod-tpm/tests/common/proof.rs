//! The proof of the crate against a TPM: the sequences of `flow` over a
//! `Transport` that is a TPM (the TPM in software of the feature `soft`,
//! swtpm, the simulator of the reference). Each check prints one line
//! that starts with the name of the TPM, so that a script of CI can see
//! that the check ran and did not skip.

use std::time::{Duration, Instant};

use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use pod_core::Class;
use pod_tpm::cmd::{self, CAP_HANDLES, Capability, CapabilityData, SessionKind, Ticket};
use pod_tpm::flow::{self, pcr, sign};
use pod_tpm::policy::{approved_policy_hash, key_name};
use pod_tpm::public::{self, NULL_SIGNATURE};
use pod_tpm::rc::{Code, Subject};
use pod_tpm::wire::{Auth, RH_OWNER};
use pod_tpm::{AuthPolicy, Authorize, EcdsaSignature, Error, HashAlg, KeyFile, PcrSelection, Policy, Statement};
use pod_tpm::{Transport, transact};

use super::fake::Bytes;

/// The policy reference of the policy key of the tests.
const POLICY_REF: [u8; 8] = *b"podssh-1";

/// The PCR selection of the tests: PCR 7 of SHA-256, the state of Secure
/// Boot.
pub fn seven() -> PcrSelection {
    PcrSelection::parse("7").unwrap()
}

/// The SHA-256 of `data`, the digest that a key signs.
pub fn digest(data: &[u8]) -> [u8; 32] {
    pod_tpm::policy::hash(HashAlg::Sha256, &[data]).unwrap().try_into().unwrap()
}

/// The transient objects and the sessions that `tpm` holds now, by
/// `TPM2_GetCapability` of the handles of each range.
pub fn live(tpm: &mut dyn Transport) -> Vec<u32> {
    let mut all = Vec::new();
    for (range, kinds) in [(0x8000_0000u32, &[0x80u32][..]), (0x0200_0000, &[0x02, 0x03]), (0x0300_0000, &[0x02, 0x03])]
    {
        let reply = transact(tpm, &cmd::get_capability(CAP_HANDLES, range, 64).unwrap()).unwrap();
        let CapabilityData::Handles(list) = Capability::read(&reply).unwrap().data else {
            panic!("the TPM gave no list of handles for the range {range:#010x}");
        };
        all.extend(list.into_iter().filter(|h| kinds.contains(&(h >> 24))));
    }
    all.sort_unstable();
    all.dedup();
    all
}

/// The check that `signature` of `key_file`'s key is a valid ECDSA P-256
/// signature of `digest`, by the `p256` crate: an oracle that is not the
/// TPM's.
pub fn verify(file: &KeyFile, digest: &[u8; 32], signature: &EcdsaSignature) -> bool {
    let point = flow::public_point(file).unwrap();
    let key = VerifyingKey::from_sec1_bytes(&point).unwrap();
    let signature = Signature::from_slice(&[signature.r, signature.s].concat()).unwrap();
    key.verify_prehash(digest, &signature).is_ok()
}

/// Load the key of `file` and sign `data` with it.
pub fn sign_with(tpm: &mut dyn Transport, file: &KeyFile, data: &[u8]) -> Result<EcdsaSignature, Error> {
    let mut key = flow::load(tpm, file)?;
    sign::sign(&mut key, &mut Bytes(2), file, &digest(data))
}

/// Extend PCR 7 with the hash of `event`, as a boot measures.
pub fn measure(tpm: &mut dyn Transport, event: &[u8]) {
    pcr::pcr_extend(tpm, 7, &digest(event)).unwrap();
}

/// A key sealed to PCR 7 signs, and `p256` verifies the signature; the
/// digest of the crate is the TPM's; after `TPM2_PCR_Extend` of PCR 7 the
/// key is refused, and a new trial session reaches another digest. No
/// handle stays.
pub fn sealed_to_pcr7(tpm: &mut dyn Transport, name: &str) {
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: a handle before the test");
    measure(tpm, b"Secure Boot: on, the vendor's keys");
    let policy = Policy::new(vec![pcr::policy_pcr_now(tpm, &seven()).unwrap()]);
    let wanted = policy.digest(HashAlg::Sha256).unwrap();
    assert_eq!(flow::trial_digest(tpm, &mut Bytes(1), &policy).unwrap(), wanted);
    println!("{name}: the policy digest of PolicyPCR is the crate's");
    let (file, area) = flow::create_signing(tpm, &mut Bytes(1), RH_OWNER, &policy).unwrap();
    assert_eq!(area.auth_policy, wanted);
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: create_signing leaves a handle");
    let signature = sign_with(tpm, &file, b"a login").unwrap();
    assert!(verify(&file, &digest(b"a login"), &signature), "{name}: the signature does not verify");
    assert!(!verify(&file, &digest(b"another login"), &signature));
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: sign leaves a handle");
    println!("{name}: a key sealed to PCR 7 signed, and p256 verified the signature");
    let file = KeyFile::from_pem(&file.to_pem().unwrap()).unwrap();
    measure(tpm, b"Secure Boot: off");
    let why = sign_with(tpm, &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Policy, "{name}: {why}");
    assert!(why.to_string().contains("the PCRs sha256:7 hold other values"), "{name}: {why}");
    let now = Policy::new(vec![pcr::policy_pcr_now(tpm, &seven()).unwrap()]);
    assert_ne!(flow::trial_digest(tpm, &mut Bytes(1), &now).unwrap(), wanted);
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: a refusal leaves a handle");
    println!("{name}: after PCR_Extend of PCR 7 the key was refused");
}

/// The policy key of the tests, of a made-up scalar, and its public area.
pub fn policy_key(byte: u8) -> (SigningKey, Vec<u8>) {
    let key = SigningKey::from_slice(&[byte; 32]).unwrap();
    let point = key.verifying_key().to_sec1_point(false);
    let public = public::external_p256(point.as_bytes()).unwrap();
    (key, public)
}

/// A signed policy of PCR 7 as it is now, signed off the TPM by `key`.
pub fn signed_now(tpm: &mut dyn Transport, key: &SigningKey, public: &[u8], name: &str) -> AuthPolicy {
    let statement = pcr::policy_pcr_now(tpm, &seven()).unwrap();
    let approved = Policy::new(vec![statement.clone()]).digest(HashAlg::Sha256).unwrap();
    let a_hash = approved_policy_hash(HashAlg::Sha256, &approved, &POLICY_REF).unwrap();
    let signed: Signature = key.sign_prehash(&a_hash).unwrap();
    let bytes = signed.to_bytes();
    let signature = EcdsaSignature { r: bytes[..32].try_into().unwrap(), s: bytes[32..].try_into().unwrap() };
    let authorize =
        Authorize { public: public.to_vec(), policy_ref: POLICY_REF.to_vec(), signature: signature.to_tpmt() };
    let steps = vec![statement, Statement::authorize(&authorize).unwrap()];
    AuthPolicy { name: Some(name.to_string()), policy: Policy::new(steps) }
}

/// A key under a policy key (`TPM2_PolicyAuthorize`) signs under the
/// signed policy of the boot state; after `TPM2_PCR_Extend` it is refused
/// with "no signed policy for this boot state"; a signed policy of the new
/// state lets it sign again. No handle stays.
pub fn signed_policies(tpm: &mut dyn Transport, name: &str) {
    measure(tpm, b"Secure Boot: on, the vendor's keys");
    let (key, public) = policy_key(0x42);
    let head =
        Authorize { public: public.clone(), policy_ref: POLICY_REF.to_vec(), signature: NULL_SIGNATURE.to_vec() };
    let own = Policy::new(vec![Statement::authorize(&head).unwrap()]);
    let wanted = own.digest(HashAlg::Sha256).unwrap();
    assert_eq!(flow::trial_digest(tpm, &mut Bytes(1), &own).unwrap(), wanted);
    println!("{name}: the policy digest of PolicyAuthorize is the crate's");
    let (file, _) = flow::create_signing(tpm, &mut Bytes(1), RH_OWNER, &own).unwrap();
    let file = sign::add_signed_policy(&file, signed_now(tpm, &key, &public, "first boot")).unwrap();
    let signature = sign_with(tpm, &file, b"a login").unwrap();
    assert!(verify(&file, &digest(b"a login"), &signature), "{name}: the signature does not verify");
    println!("{name}: a key under a policy key signed in the signed boot state");
    measure(tpm, b"the firmware's update");
    let why = sign_with(tpm, &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Policy, "{name}: {why}");
    assert!(why.to_string().contains(sign::NO_SIGNED_POLICY), "{name}: {why}");
    println!("{name}: after PCR_Extend the old signed policy was refused");
    let new = sign::add_signed_policy(&file, signed_now(tpm, &key, &public, "firmware 2")).unwrap();
    let signature = sign_with(tpm, &new, b"a login").unwrap();
    assert!(verify(&new, &digest(b"a login"), &signature), "{name}: the signature does not verify");
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: a handle stays");
    println!("{name}: a signed policy of the new state let the key sign again");
}

/// A key file of `first` does not load in `second`, another TPM.
pub fn copy_elsewhere(first: &mut dyn Transport, second: &mut dyn Transport, name: &str) {
    let (file, _) = made(first);
    sign_with(first, &file, b"a login").unwrap();
    let why = flow::load(second, &file).map(drop).unwrap_err();
    assert_eq!(why.class(), Class::Policy, "{name}: {why}");
    assert!(why.to_string().contains("this TPM cannot load the key"), "{name}: {why}");
    assert_eq!(live(second), Vec::<u32>::new(), "{name}: a refused load leaves a handle");
    println!("{name}: a copy of the key file did not load in a second TPM");
}

/// A key that the crate made, under a policy of PCR 7.
fn made(tpm: &mut dyn Transport) -> (KeyFile, Policy) {
    let policy = Policy::new(vec![pcr::policy_pcr_now(tpm, &seven()).unwrap()]);
    let (file, _) = flow::create_signing(tpm, &mut Bytes(1), RH_OWNER, &policy).unwrap();
    (file, policy)
}

/// The TPM refuses the password session for a key under a policy (the key
/// has no `userWithAuth`), and the crate refuses a policy session that
/// reached another digest than the key's before `TPM2_Sign`.
pub fn signer_ignores_policy(tpm: &mut dyn Transport, name: &str) {
    let (file, _) = made(tpm);
    let mut key = flow::load(tpm, &file).unwrap();
    let handle = key.handle();
    let why = transact(key.transport(), &cmd::sign(handle, &Auth::EMPTY_PASSWORD, &digest(b"x")).unwrap()).unwrap_err();
    assert_eq!(why.code().map(|c| c.code()), Some(Code::AuthUnavailable), "{name}: {why}");
    key.finish().unwrap();
    println!("{name}: a signature by the password of a key under a policy was refused");
    let mut other = file.clone();
    let zero = PcrSelection::parse("0").unwrap();
    other.policy = Policy::new(vec![pcr::policy_pcr_now(tpm, &zero).unwrap()]);
    let why = sign_with(tpm, &other, b"a login").unwrap_err();
    assert!(why.to_string().contains("another digest than the authPolicy"), "{name}: {why}");
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: a handle stays");
    println!("{name}: a policy of other PCRs than the key's was refused");
}

/// `TPM2_PolicyAuthorize` with the ticket of nothing outside a trial
/// session: `TPM_RC_VALUE` of parameter 4, the ticket (part 3, 23.16).
pub fn null_ticket(tpm: &mut dyn Transport, name: &str) {
    let (_, policy) = made(tpm);
    let (_, public) = policy_key(0x42);
    let approved = policy.digest(HashAlg::Sha256).unwrap();
    let (mut session, _) = flow::session(tpm, &mut Bytes(3), SessionKind::Policy).unwrap();
    let (selection, values) = policy.statements[0].read_pcr().unwrap();
    let handle = session.handle();
    transact(session.transport(), &cmd::policy_pcr(handle, &values, &selection).unwrap()).unwrap().empty().unwrap();
    let command =
        cmd::policy_authorize(handle, &approved, &POLICY_REF, &key_name(&public).unwrap(), &Ticket::null()).unwrap();
    let why = transact(session.transport(), &command).unwrap_err();
    let code = why.code().unwrap();
    assert_eq!((code.code(), code.subject()), (Code::Value, Some(Subject::Parameter(4))), "{name}: {why}");
    session.finish().unwrap();
    assert_eq!(live(tpm), Vec::<u32>::new(), "{name}: a handle stays");
    println!("{name}: a ticket of nothing was refused outside a trial");
}

/// The capabilities of a doctor line: a manufacturer, the algorithms of
/// the key, and a free transient slot.
pub fn capabilities(tpm: &mut dyn Transport, name: &str) {
    let facts = pcr::capabilities(tpm).unwrap();
    let manufacturer = facts.manufacturer.clone().unwrap_or_default();
    assert!(!manufacturer.is_empty(), "{name}: no manufacturer");
    for alg in [pod_tpm::alg::ALG_SHA256, pod_tpm::alg::ALG_ECC, pod_tpm::alg::ALG_ECDSA] {
        assert!(facts.algorithms.contains(&alg), "{name}: no algorithm {alg:#06x} in {:?}", facts.algorithms);
    }
    assert!(facts.transient_available.is_some_and(|n| n >= 3), "{name}: {facts:?}");
    println!("{name}: the capabilities name the TPM ({manufacturer})");
}

/// Two `TPM2_CreatePrimary` of the storage primary fit in a tenth of
/// `limit`, the time limit of a command of the transport.
pub fn primary_fits(tpm: &mut dyn Transport, limit: Duration, name: &str) {
    let (first, again) = (time_primary(tpm), time_primary(tpm));
    assert!(first.max(again) < limit / 10, "{name}: TPM2_CreatePrimary took {first:?} and {again:?}");
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{name}: TPM2_CreatePrimary of the storage primary took {:.3} ms and {:.3} ms, in a limit of {} s",
        ms(first),
        ms(again),
        limit.as_secs()
    );
}

/// The time of `TPM2_CreatePrimary` of the storage primary H-2 (P-256),
/// the slowest command of the sequences.
fn time_primary(tpm: &mut dyn Transport) -> Duration {
    let start = Instant::now();
    let (guard, primary) = flow::primary(tpm, RH_OWNER).unwrap();
    let took = start.elapsed();
    assert_eq!(primary.name.len(), 34, "a name of SHA-256");
    guard.finish().unwrap();
    took
}
