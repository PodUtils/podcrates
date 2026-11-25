//! The sequences of `flow` against the TPM in software (feature `soft`),
//! with the checks of `common::proof` that also run against swtpm and the
//! simulator of the reference; and what the model itself must hold: each
//! field of each response, the handles of a TPM, the refusals of a TPM.
#![cfg(feature = "soft")]

mod common;

use common::fake::Bytes;
use common::proof::{self, digest, live, seven};
use pod_core::Class;
use pod_tpm::cmd::{self, CC_CREATE_PRIMARY, Primary};
use pod_tpm::flow::{self, pcr};
use pod_tpm::rc::Code;
use pod_tpm::soft::SoftTpm;
use pod_tpm::wire::{In, RH_OWNER, ST_SESSIONS};
use pod_tpm::{Error, Policy, Transport, public, transact};

const NAME: &str = "the TPM in software";

fn tpm(seed: u8) -> SoftTpm {
    SoftTpm::new([seed; 32])
}

#[test]
fn a_sealed_key_signs_while_the_boot_state_is_the_recorded_one() {
    let mut tpm = tpm(1);
    proof::sealed_to_pcr7(&mut tpm, NAME);
    assert_eq!(tpm.loaded(), 0);
}

#[test]
fn after_a_signed_policy_for_the_new_state_the_key_signs_again() {
    let mut tpm = tpm(1);
    proof::signed_policies(&mut tpm, NAME);
    assert_eq!(tpm.loaded(), 0);
}

#[test]
fn a_copy_of_the_key_file_does_not_load_in_another_tpm() {
    proof::copy_elsewhere(&mut tpm(1), &mut tpm(2), NAME);
}

#[test]
fn a_signer_that_ignores_the_policy_is_refused() {
    proof::signer_ignores_policy(&mut tpm(1), NAME);
}

#[test]
fn a_ticket_of_nothing_is_refused_outside_a_trial() {
    proof::null_ticket(&mut tpm(1), NAME);
}

#[test]
fn the_capabilities_name_the_tpm() {
    proof::capabilities(&mut tpm(1), NAME);
}

/// The same seed is the same TPM: a key file of one loads in the other, so
/// a consumer's test can keep a key file as a vector.
#[test]
fn the_seed_makes_the_same_tpm() {
    let (mut first, mut second) = (tpm(5), tpm(5));
    first.extend(7, b"a boot");
    second.extend(7, b"a boot");
    let policy = Policy::new(vec![pcr::policy_pcr_now(&mut first, &seven()).unwrap()]);
    let (file, _) = flow::create_signing(&mut first, &mut Bytes(1), RH_OWNER, &policy).unwrap();
    let signature = proof::sign_with(&mut second, &file, b"a login").unwrap();
    assert!(proof::verify(&file, &digest(b"a login"), &signature));
    assert_eq!(first.pcr(7), second.pcr(7));
    assert_ne!(first.pcr(7), [0; 32], "extend changed PCR 7");
}

/// A key under the persistent storage key `0x81000001`: no primary is made.
#[test]
fn a_persistent_parent_is_used_as_it_is() {
    let mut tpm = tpm(1);
    tpm.persist(0x8100_0001);
    let policy = Policy::new(vec![pcr::policy_pcr_now(&mut tpm, &seven()).unwrap()]);
    let (file, _) = flow::create_signing(&mut tpm, &mut Bytes(1), 0x8100_0001, &policy).unwrap();
    proof::sign_with(&mut tpm, &file, b"a login").unwrap();
    let primaries = tpm.commands().iter().filter(|c| c[6..10] == CC_CREATE_PRIMARY.to_be_bytes()).count();
    assert_eq!(primaries, 0, "a primary was made under a persistent parent");
    let why = proof::sign_with(&mut self::tpm(1), &file, b"a login").unwrap_err();
    assert_eq!(why.class(), Class::Unavailable, "{why}");
    assert!(why.to_string().contains("0x81000001"), "{why}");
}

/// The response of `TPM2_CreatePrimary` read field by field, as part 3
/// (24.1) gives it, and the area of the session after its parameters.
#[test]
fn each_field_of_a_response_is_there() {
    let mut tpm = tpm(1);
    let command = cmd::create_primary(RH_OWNER, &public::storage_primary().unwrap()).unwrap();
    let bytes = tpm.submit(&command).unwrap();
    let mut r = In::new(&bytes);
    assert_eq!(r.u16().unwrap(), ST_SESSIONS);
    assert_eq!(r.u32().unwrap() as usize, bytes.len());
    assert_eq!(r.u32().unwrap(), 0, "TPM_RC_SUCCESS");
    let handle = r.u32().unwrap();
    assert_eq!(handle >> 24, 0x80, "a transient handle");
    let size = r.u32().unwrap() as usize;
    let mut p = In::new(r.take(size).unwrap());
    let area = pod_tpm::EccPublic::read(p.sized_whole().unwrap()).unwrap();
    assert_eq!(area.attributes, public::STORAGE);
    let mut creation = In::new(p.sized().unwrap());
    assert_eq!(creation.u32().unwrap(), 0, "no PCRs in the creation data");
    assert_eq!(creation.sized().unwrap().len(), 32, "pcrDigest");
    assert_eq!(creation.u8().unwrap(), 1, "locality 0");
    assert_eq!(creation.u16().unwrap(), 0x0010, "the parent of a primary has no name algorithm");
    assert_eq!(creation.sized().unwrap(), RH_OWNER.to_be_bytes(), "parentName");
    assert_eq!(creation.sized().unwrap(), RH_OWNER.to_be_bytes(), "parentQualifiedName");
    assert_eq!(creation.sized().unwrap(), b"", "outsideInfo");
    creation.finish("creationData").unwrap();
    assert_eq!(p.sized().unwrap().len(), 32, "creationHash");
    assert_eq!((p.u16().unwrap(), p.u32().unwrap()), (0x8021, RH_OWNER), "creationTicket");
    assert_eq!(p.sized().unwrap().len(), 32, "the digest of the ticket");
    assert_eq!(p.sized().unwrap().len(), 34, "the name");
    p.finish("the parameters").unwrap();
    assert_eq!((r.sized().unwrap(), r.u8().unwrap(), r.sized().unwrap()), (&[][..], 1, &[][..]), "the session");
    r.finish("the response").unwrap();
    let reply = pod_tpm::Reply::read(CC_CREATE_PRIMARY, &bytes).unwrap();
    let primary = Primary::read(&reply).unwrap();
    assert_eq!(primary.name, pod_tpm::policy::key_name(&primary.public).unwrap());
    flow::flush(&mut tpm, handle).unwrap();
    assert_eq!(tpm.loaded(), 0);
}

/// The model refuses what a TPM refuses, with the code of a TPM: a command
/// that it does not know, a size that is not the size of the command, a
/// command of a hierarchy with no session, a byte after the parameters, a
/// fourth transient object, a flush of a handle that is not loaded.
#[test]
fn the_tpm_in_software_refuses_as_a_tpm() {
    let mut tpm = tpm(1);
    let code = |tpm: &mut SoftTpm, command: &[u8]| match transact(tpm, command) {
        Err(Error::Tpm(code)) => code.code(),
        other => panic!("not a refusal of the TPM: {other:?}"),
    };
    let mut unknown = cmd::flush_context(0x8000_0001).unwrap();
    unknown[6..10].copy_from_slice(&0x0000_0144u32.to_be_bytes());
    assert_eq!(code(&mut tpm, &unknown), Code::CommandCode, "TPM2_Startup is not a command of the model");
    let mut long = cmd::flush_context(0x8000_0001).unwrap();
    long[5] += 1;
    assert_eq!(code(&mut tpm, &long), Code::CommandSize);
    // PCR_Extend with its authorization area cut away, as a command of no
    // session.
    let mut bare = cmd::pcr_extend(7, &[1; 32]).unwrap();
    bare.drain(14..27);
    bare[0..2].copy_from_slice(&0x8001u16.to_be_bytes());
    assert_eq!(code(&mut tpm, &sized(bare)), Code::AuthMissing);
    let mut trailing = cmd::get_manufacturer().unwrap();
    trailing.push(0);
    assert_eq!(code(&mut tpm, &sized(trailing)), Code::Size);
    assert_eq!(code(&mut tpm, &cmd::flush_context(0x8000_0042).unwrap()), Code::Handle);
    let template = public::storage_primary().unwrap();
    let mut guards = Vec::new();
    for _ in 0..3 {
        let reply = transact(&mut tpm, &cmd::create_primary(RH_OWNER, &template).unwrap()).unwrap();
        guards.push(reply.handle().unwrap());
    }
    let full = transact(&mut tpm, &cmd::create_primary(RH_OWNER, &template).unwrap()).unwrap_err();
    assert_eq!((full.code().unwrap().code(), full.class()), (Code::ObjectMemory, Class::TempFail));
    assert_eq!(live(&mut tpm), guards);
    for handle in guards {
        flow::flush(&mut tpm, handle).unwrap();
    }
    assert_eq!(tpm.loaded(), 0);
}

/// `command` with its size field set to its length.
fn sized(mut command: Vec<u8>) -> Vec<u8> {
    let size = (command.len() as u32).to_be_bytes();
    command[2..6].copy_from_slice(&size);
    command
}
