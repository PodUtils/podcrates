//! The sequences against a fake TPM (`common::fake`): a key is made and
//! signs; after the boot state changed it does not; a copy in another TPM
//! does not load; a persistent parent is used as it is; and each path,
//! also each failure of each step, leaves no handle loaded.

mod common;

use common::fake::{Bytes, Fail, FakeTpm, RC_FAILURE};
use pod_core::Class;
use pod_tpm::cmd::{CC_CREATE, CC_CREATE_PRIMARY, CC_FLUSH_CONTEXT, CC_PCR_READ, CC_POLICY_AUTHORIZE, CC_SIGN};
use pod_tpm::flow::{self, Handle, pcr, sign};
use pod_tpm::public::NULL_SIGNATURE;
use pod_tpm::wire::RH_OWNER;
use pod_tpm::{AuthPolicy, Authorize, EcdsaSignature, Error, HashAlg, KeyFile, PcrSelection, Policy, Statement};

fn sel(text: &str) -> PcrSelection {
    PcrSelection::parse(text).unwrap()
}

/// A TPM whose PCR 7 measured a boot, and a key under `parent` sealed to
/// the PCRs of `pcrs` as they are now.
fn made(pcrs: &str, parent: u32) -> (FakeTpm, KeyFile) {
    let mut tpm = FakeTpm::new();
    tpm.extend(7, b"Secure Boot: on, the vendor's keys");
    let statement = pcr::policy_pcr_now(&mut tpm, &sel(pcrs)).unwrap();
    let (mut file, area) =
        flow::create_signing(&mut tpm, &mut Bytes(1), parent, &Policy::new(vec![statement])).unwrap();
    assert_eq!(area.point, flow::public_point(&file).unwrap());
    assert_eq!(tpm.loaded(), 0, "make leaves no handle");
    file.description = Some("me@lab".into());
    (tpm, file)
}

/// Load the key of `file` and sign `data` with it.
fn try_sign(tpm: &mut FakeTpm, file: &KeyFile, data: &[u8]) -> Result<pod_tpm::EcdsaSignature, Error> {
    let digest: [u8; 32] = pod_tpm::policy::hash(pod_tpm::HashAlg::Sha256, &[data]).unwrap().try_into().unwrap();
    flow::load(tpm, file).and_then(|mut key: Handle<'_>| sign::sign(&mut key, &mut Bytes(2), file, &digest))
}

/// Load the key of `file` and sign `data` with it; no handle stays.
fn signed(tpm: &mut FakeTpm, file: &KeyFile, data: &[u8]) -> Result<pod_tpm::EcdsaSignature, Error> {
    let result = try_sign(tpm, file, data);
    assert_eq!(tpm.loaded(), 0, "sign leaves no handle: {result:?}");
    assert_eq!(tpm.bad_flushes, 0, "no handle is flushed twice");
    result
}

#[test]
fn a_sealed_key_signs_while_the_boot_state_is_the_recorded_one() {
    let (mut tpm, file) = made("7", RH_OWNER);
    let signature = signed(&mut tpm, &file, b"a login").unwrap();
    assert_eq!((signature.r, signature.s), ([0x11; 32], [0x22; 32]));
    // The file says what it is sealed to, as other readers read it.
    assert_eq!(file.policy.statements.len(), 1);
    assert_eq!(file.parent, RH_OWNER);
    assert!(file.empty_auth);
    let again = KeyFile::from_pem(&file.to_pem().unwrap()).unwrap();
    assert_eq!(again, file);
    assert_eq!(
        tpm.given,
        tpm.count(CC_FLUSH_CONTEXT) + 1,
        "each handle flushed once; the session ended by the signature"
    );
}

#[test]
fn after_the_boot_state_changed_the_key_is_refused_in_one_line() {
    let (mut tpm, file) = made("0,7", RH_OWNER);
    tpm.extend(7, b"Secure Boot: off");
    let why = signed(&mut tpm, &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Policy);
    let line = why.to_string();
    assert!(line.contains("the PCRs sha256:0,7 hold other values"), "{line}");
    assert!(!line.contains('\n'));
    // A PCR that the key is not sealed to changes nothing.
    let (mut tpm, file) = made("7", RH_OWNER);
    tpm.extend(11, b"a kernel's update");
    signed(&mut tpm, &file, b"a login").unwrap();
}

#[test]
fn a_copy_of_the_key_file_does_not_load_in_another_tpm() {
    let (tpm, file) = made("7", RH_OWNER);
    let mut other = FakeTpm::new();
    other.pcrs = tpm.pcrs;
    other.seed = 2;
    let why = signed(&mut other, &file, b"a login").unwrap_err();
    assert!(why.to_string().contains("this TPM cannot load the key"), "{why}");
    assert_eq!(why.class(), Class::Policy);
}

/// A TPM whose trial reaches another digest than the crate's: no key is
/// made.
#[test]
fn a_policy_that_the_tpm_reads_otherwise_makes_no_key() {
    let mut tpm = FakeTpm::new();
    tpm.other_trial = true;
    let statement = pcr::policy_pcr_now(&mut tpm, &sel("7")).unwrap();
    let why = flow::create_signing(&mut tpm, &mut Bytes(1), RH_OWNER, &Policy::new(vec![statement])).unwrap_err();
    assert!(why.to_string().contains("not the crate's"), "{why}");
    assert_eq!(tpm.count(CC_CREATE), 0, "no TPM2_Create was sent");
    assert_eq!(tpm.loaded(), 0);
}

/// More than 8 PCRs take two reads, and their values come in order.
#[test]
fn many_pcrs_are_read_in_two_reads() {
    let mut tpm = FakeTpm::new();
    for n in 0..12 {
        tpm.extend(n, &[n as u8]);
    }
    let values = pcr::pcr_read(&mut tpm, &sel("0,1,2,3,4,5,6,7,8,9,10,11")).unwrap();
    let want: Vec<Vec<u8>> = tpm.pcrs[..12].iter().map(|v| v.to_vec()).collect();
    assert_eq!(values, want);
    assert_eq!(tpm.count(CC_PCR_READ), 2);
    pcr::pcr_extend(&mut tpm, 3, &[9; 32]).unwrap();
    assert_ne!(pcr::pcr_read(&mut tpm, &sel("3")).unwrap()[0], want[3]);
}

/// The Sign command carries the policy session, with continueSession
/// clear and no HMAC, and a nonce of 32 bytes.
#[test]
fn the_signature_is_asked_in_the_policy_session() {
    let (mut tpm, file) = made("7", RH_OWNER);
    tpm.sent.clear();
    signed(&mut tpm, &file, b"a login").unwrap();
    let signs: Vec<&Vec<u8>> = tpm.sent.iter().filter(|c| c[6..10] == CC_SIGN.to_be_bytes()).collect();
    assert_eq!(signs.len(), 1);
    let sign = signs[0];
    assert_eq!(&sign[0..2], &[0x80, 0x02], "with sessions");
    // The header, the key's handle, the area's size, then the session.
    assert_eq!(sign[18], 0x03, "a policy session's handle");
    assert_eq!(&sign[22..24], &[0, 32], "a nonce of 32 bytes");
    assert_eq!(&sign[56..59], &[0, 0, 0], "continueSession clear, no HMAC");
}

/// A key under the persistent storage key: no primary is made, to make or
/// to load it. A TPM with no such key says so, with the handle.
#[test]
fn a_persistent_parent_is_used_as_it_is() {
    let (mut tpm, file) = made("7", 0x8100_0001);
    assert_eq!(tpm.count(CC_CREATE_PRIMARY), 0);
    assert_eq!(file.parent, 0x8100_0001);
    signed(&mut tpm, &file, b"a login").unwrap();
    assert_eq!(tpm.count(CC_CREATE_PRIMARY), 0);
    tpm.persistent.clear();
    let why = signed(&mut tpm, &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Unavailable);
    assert!(why.to_string().contains("0x81000001"), "{why}");
}

/// A file of an RSA parent, a key that needs a password and a key with no
/// policy are refused before a command goes to the TPM.
#[test]
fn a_key_that_the_crate_does_not_sign_with_is_refused() {
    let (mut tpm, file) = made("7", RH_OWNER);
    let mut rsa = file.clone();
    rsa.rsa_parent = true;
    tpm.sent.clear();
    assert!(matches!(flow::load(&mut tpm, &rsa), Err(Error::Unsupported(_))));
    assert!(tpm.sent.is_empty());
    let mut password = file.clone();
    password.empty_auth = false;
    assert!(matches!(signed(&mut tpm, &password, b"x"), Err(Error::Unsupported(_))));
    let mut bare = file.clone();
    bare.policy = Policy::default();
    assert!(signed(&mut tpm, &bare, b"x").unwrap_err().to_string().contains("no policy"));
}

/// The capabilities of a doctor line.
#[test]
fn the_capabilities_name_the_tpm() {
    let mut tpm = FakeTpm::new();
    let facts = pcr::capabilities(&mut tpm).unwrap();
    assert_eq!(facts.manufacturer.as_deref(), Some("FBK"));
    assert_eq!(facts.algorithms, vec![0x0B, 0x18, 0x23]);
    assert_eq!(facts.transient_available, Some(3));
}

/// Each command of the making, of the load and of the signature fails in
/// turn: refused by the TPM, lost by the transport, or answered with its
/// handles alone. Each handle that the TPM gave is flushed once, on each
/// path. A failed flush itself leaves its handle, which the guard can only
/// log: those commands are not failed.
#[test]
fn each_failed_step_flushes_each_live_handle() {
    let (clean, file) = made("7", RH_OWNER);
    let make_steps = clean.sent.clone();
    let mut tpm = FakeTpm::with(clean.pcrs, None);
    signed(&mut tpm, &file, b"a login").unwrap();
    let sign_steps = tpm.sent.clone();
    let policy = file.policy.clone();
    for fail in [Fail::Tpm(RC_FAILURE), Fail::Transport, Fail::Short] {
        // The first command of `made` reads the PCRs; the making starts after it.
        for at in steps(&make_steps).filter(|at| *at > 0) {
            let mut tpm = FakeTpm::with(clean.pcrs, Some((at - 1, fail)));
            let result = flow::create_signing(&mut tpm, &mut Bytes(1), RH_OWNER, &policy);
            check(&tpm, at, fail, result.map(drop));
        }
        for at in steps(&sign_steps) {
            let mut tpm = FakeTpm::with(clean.pcrs, Some((at, fail)));
            let result = try_sign(&mut tpm, &file, b"a login");
            check(&tpm, at, fail, result.map(drop));
        }
    }
}

/// The indices of the commands of a clean run that are no flush.
pub fn steps(sent: &[Vec<u8>]) -> impl Iterator<Item = usize> + '_ {
    (0..sent.len()).filter(|at| sent[*at][6..10] != CC_FLUSH_CONTEXT.to_be_bytes())
}

/// After the failure `fail` at the command `at`: an error, but for a
/// response of handles alone to a command that gives no parameter; no
/// handle loaded, none flushed twice.
pub fn check(tpm: &FakeTpm, at: usize, fail: Fail, result: Result<(), Error>) {
    if fail != Fail::Short {
        assert!(result.is_err(), "the failure at {at} ({fail:?}) gave no error");
    }
    assert_eq!(tpm.loaded(), 0, "a handle stays after the failure at {at} ({fail:?}): {result:?}");
    assert_eq!(tpm.bad_flushes, 0, "a flush of a handle that is not loaded, at {at} ({fail:?})");
}

/// The policy key of the tests: a made-up point of P-256.
fn policy_key(byte: u8) -> Vec<u8> {
    let point = common::unhex(&format!("04 {} {}", format!("{byte:02x}").repeat(32), "22".repeat(32)));
    pod_tpm::public::external_p256(&point).unwrap()
}

/// A signed policy of the PCRs of `pcrs` as they are now, signed by the
/// policy key `key`: the fake takes a signature whose `r` is the digest.
fn signed_policy(tpm: &mut FakeTpm, pcrs: &str, key: &[u8], name: Option<&str>) -> AuthPolicy {
    let statement = pcr::policy_pcr_now(tpm, &sel(pcrs)).unwrap();
    let approved = Policy::new(vec![statement.clone()]).digest(HashAlg::Sha256).unwrap();
    let a_hash = pod_tpm::policy::approved_policy_hash(HashAlg::Sha256, &approved, &[9; 32]).unwrap();
    let signature = EcdsaSignature { r: a_hash.try_into().unwrap(), s: [1; 32] }.to_tpmt();
    let authorize = Authorize { public: key.to_vec(), policy_ref: vec![9; 32], signature };
    let steps = vec![statement, Statement::authorize(&authorize).unwrap()];
    AuthPolicy { name: name.map(str::to_string), policy: Policy::new(steps) }
}

/// A TPM whose PCR 7 measured a boot, and a key under the policy key, with
/// a signed policy of the boot state of now.
fn made_signed() -> (FakeTpm, KeyFile) {
    let mut tpm = FakeTpm::new();
    tpm.extend(7, b"Secure Boot: on, the vendor's keys");
    let head = Authorize { public: policy_key(0x11), policy_ref: vec![9; 32], signature: NULL_SIGNATURE.to_vec() };
    let own = Policy::new(vec![Statement::authorize(&head).unwrap()]);
    let (file, _) = flow::create_signing(&mut tpm, &mut Bytes(1), RH_OWNER, &own).unwrap();
    let first = signed_policy(&mut tpm, "7", &policy_key(0x11), None);
    let file = sign::add_signed_policy(&file, first).unwrap();
    assert_eq!(tpm.loaded(), 0);
    (tpm, file)
}

#[test]
fn after_a_signed_policy_for_the_new_state_the_key_signs_again() {
    let (mut tpm, file) = made_signed();
    assert_eq!(file.policy.statements.len(), 1);
    assert_eq!(file.policy.statements[0].code, CC_POLICY_AUTHORIZE);
    signed(&mut tpm, &file, b"a login").unwrap();
    tpm.extend(7, b"the firmware's update");
    let why = signed(&mut tpm, &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Policy);
    assert!(why.to_string().contains(sign::NO_SIGNED_POLICY), "{why}");
    assert!(why.to_string().contains("the PCRs sha256:7 hold other values"), "{why}");
    assert!(!why.to_string().contains('\n'));
    let new =
        sign::add_signed_policy(&file, signed_policy(&mut tpm, "7", &policy_key(0x11), Some("firmware 2"))).unwrap();
    assert_eq!(new.auth_policy.len(), 2);
    assert_eq!(new.auth_policy[0].name.as_deref(), Some("firmware 2"));
    assert_eq!((&new.public, &new.private, &new.policy), (&file.public, &file.private, &file.policy), "the same key");
    signed(&mut tpm, &new, b"a login").unwrap();
    // The same state signed again replaces its policy.
    let again = sign::add_signed_policy(&new, signed_policy(&mut tpm, "7", &policy_key(0x11), None)).unwrap();
    assert_eq!(again.auth_policy.len(), 2);
    assert_eq!(again.auth_policy[1], new.auth_policy[1]);
    assert_eq!(KeyFile::from_pem(&new.to_pem().unwrap()).unwrap(), new);
}

#[test]
fn a_policy_of_another_key_or_a_changed_signature_is_refused() {
    let (mut tpm, file) = made_signed();
    let other = signed_policy(&mut tpm, "7", &policy_key(0x33), None);
    let why = sign::add_signed_policy(&file, other.clone()).unwrap_err();
    assert!(why.to_string().contains("not the one that the key file names"), "{why}");
    // A writer of the file puts a policy of its own key: refused, and the
    // next signed policy is tried.
    let mut forged = file.clone();
    forged.auth_policy.insert(0, other);
    let signature = signed(&mut tpm, &forged, b"a login");
    assert!(signature.is_ok(), "{signature:?}");
    forged.auth_policy.truncate(1);
    let why = signed(&mut tpm, &forged, b"a login").unwrap_err();
    assert!(why.to_string().contains("another policy key"), "{why}");
    // It keeps the policy key and changes the signature: the TPM's check
    // of the signature refuses it.
    let mut changed = file.clone();
    let last = changed.auth_policy[0].policy.statements.last_mut().unwrap();
    let mut authorize = Authorize::parse(&last.body).unwrap();
    authorize.signature = EcdsaSignature { r: [5; 32], s: [1; 32] }.to_tpmt();
    last.body = authorize.to_bytes().unwrap();
    let why = signed(&mut tpm, &changed, b"a login").unwrap_err();
    assert!(why.to_string().contains("not the policy key's"), "{why}");
    let mut none = file.clone();
    none.auth_policy.clear();
    assert!(signed(&mut tpm, &none, b"a login").unwrap_err().to_string().contains("holds no signed policy"));
}

/// A key under its PCRs alone takes no signed policy.
#[test]
fn a_key_must_take_signed_policies() {
    let (mut tpm, plain) = made("7", RH_OWNER);
    let signed = signed_policy(&mut tpm, "7", &policy_key(0x11), None);
    let why = sign::add_signed_policy(&plain, signed).unwrap_err();
    assert!(why.to_string().contains("PCRs alone") && !why.to_string().contains('\n'), "{why}");
}

/// Each command of the signature with a signed policy fails in turn: each
/// handle (the key, the session, the policy key) is flushed once.
#[test]
fn each_failed_step_of_a_signed_policy_flushes_each_live_handle() {
    let (clean, file) = made_signed();
    let mut tpm = FakeTpm::with(clean.pcrs, None);
    signed(&mut tpm, &file, b"a login").unwrap();
    let sent = tpm.sent.clone();
    assert_eq!(tpm.given, tpm.count(CC_FLUSH_CONTEXT) + 1);
    for fail in [Fail::Tpm(RC_FAILURE), Fail::Transport, Fail::Short] {
        for at in steps(&sent) {
            let mut tpm = FakeTpm::with(clean.pcrs, Some((at, fail)));
            let result = try_sign(&mut tpm, &file, b"a login");
            check(&tpm, at, fail, result.map(drop));
        }
    }
}
