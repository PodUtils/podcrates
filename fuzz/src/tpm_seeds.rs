//! The seeds of the fuzz targets `tpm_response` and `tpm_keyfile`: the
//! responses of the TPM in software of `pod-tpm` (feature `soft`) to the
//! sequences of `flow`, in the form of the target (the index of the command
//! in `COMMANDS`, then the response), and the key files that the sequences
//! make, with the two vectors of other writers of
//! `crates/pod-tpm/tests/testdata/`. The seeds on disk are the ones that
//! this file makes. Write them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_tpm_seeds -- --ignored`.

use std::path::PathBuf;

use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use pod_core::Failure;
use pod_core::entropy::{Entropy, NoRandom};
use pod_tpm::cmd::{self, CAP_HANDLES};
use pod_tpm::flow::{self, pcr, sign};
use pod_tpm::keyfile::{BEGIN, END};
use pod_tpm::policy::approved_policy_hash;
use pod_tpm::public::{self, NULL_SIGNATURE};
use pod_tpm::soft::SoftTpm;
use pod_tpm::wire::RH_OWNER;
use pod_tpm::{AuthPolicy, Authorize, EcdsaSignature, HashAlg, KeyFile, PcrSelection, Policy, Statement};
use pod_tpm::{Transport, transact};

use crate::targets::COMMANDS;

/// The same byte in each nonce: the seeds do not change from run to run.
struct Fixed(u8);

impl Entropy for Fixed {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        buf.fill(self.0);
        Ok(())
    }
}

const POLICY_REF: &[u8] = b"podssh-1";

/// The SHA-256 of `data`.
fn digest(data: &[u8]) -> [u8; 32] {
    pod_tpm::policy::hash(HashAlg::Sha256, &[data]).unwrap().try_into().unwrap()
}

/// A signed policy of PCR 7 as it is now, signed by `key` off the TPM.
fn signed_now(tpm: &mut dyn Transport, key: &SigningKey, public: &[u8]) -> AuthPolicy {
    let statement = pcr::policy_pcr_now(tpm, &PcrSelection::parse("7").unwrap()).unwrap();
    let approved = Policy::new(vec![statement.clone()]).digest(HashAlg::Sha256).unwrap();
    let signed: Signature =
        key.sign_prehash(&approved_policy_hash(HashAlg::Sha256, &approved, POLICY_REF).unwrap()).unwrap();
    let bytes = signed.to_bytes();
    let signature = EcdsaSignature { r: bytes[..32].try_into().unwrap(), s: bytes[32..].try_into().unwrap() };
    let authorize =
        Authorize { public: public.to_vec(), policy_ref: POLICY_REF.to_vec(), signature: signature.to_tpmt() };
    let steps = vec![statement, Statement::authorize(&authorize).unwrap()];
    AuthPolicy { name: Some("first boot".into()), policy: Policy::new(steps) }
}

/// The sequences against the TPM in software: a key under PCR 7 and a key
/// under a policy key, each made and used to sign, a refusal after an
/// extend of PCR 7, and the capabilities. Each command and its response,
/// in order, and the two key files.
fn run() -> (Vec<(u32, Vec<u8>)>, KeyFile, KeyFile) {
    let mut tpm = SoftTpm::new([7; 32]);
    let mut seen = Vec::new();
    let mut record = |command: &[u8]| -> Result<Vec<u8>, Failure> {
        let answer = tpm.submit(command)?;
        seen.push((u32::from_be_bytes(command[6..10].try_into().unwrap()), answer.clone()));
        Ok(answer)
    };
    let seven = PcrSelection::parse("7").unwrap();
    pcr::pcr_extend(&mut record, 7, &digest(b"Secure Boot: on")).unwrap();
    let policy = Policy::new(vec![pcr::policy_pcr_now(&mut record, &seven).unwrap()]);
    let (sealed, _) = flow::create_signing(&mut record, &mut Fixed(1), RH_OWNER, &policy).unwrap();
    let mut key = flow::load(&mut record, &sealed).unwrap();
    sign::sign(&mut key, &mut Fixed(2), &sealed, &digest(b"a login")).unwrap();
    drop(key);

    let policy_key = SigningKey::from_slice(&[0x42; 32]).unwrap();
    let point = policy_key.verifying_key().to_sec1_point(false);
    let area = public::external_p256(point.as_bytes()).unwrap();
    let head = Authorize { public: area.clone(), policy_ref: POLICY_REF.to_vec(), signature: NULL_SIGNATURE.to_vec() };
    let own = Policy::new(vec![Statement::authorize(&head).unwrap()]);
    let (file, _) = flow::create_signing(&mut record, &mut Fixed(3), RH_OWNER, &own).unwrap();
    let signed = sign::add_signed_policy(&file, signed_now(&mut record, &policy_key, &area)).unwrap();
    let mut key = flow::load(&mut record, &signed).unwrap();
    sign::sign(&mut key, &mut Fixed(4), &signed, &digest(b"a login")).unwrap();
    drop(key);

    pcr::pcr_extend(&mut record, 7, &digest(b"Secure Boot: off")).unwrap();
    let mut key = flow::load(&mut record, &sealed).unwrap();
    assert!(sign::sign(&mut key, &mut Fixed(5), &sealed, &digest(b"a login")).is_err());
    drop(key);
    pcr::capabilities(&mut record).unwrap();
    transact(&mut record, &cmd::get_capability(CAP_HANDLES, 0x8000_0000, 64).unwrap()).unwrap();
    (seen, sealed, signed)
}

/// The name of the seed of the first response to each command.
fn name(code: u32) -> &'static str {
    match code {
        cmd::CC_CREATE_PRIMARY => "create_primary",
        cmd::CC_CREATE => "create",
        cmd::CC_LOAD => "load",
        cmd::CC_LOAD_EXTERNAL => "load_external",
        cmd::CC_SIGN => "sign",
        cmd::CC_VERIFY_SIGNATURE => "verify_signature",
        cmd::CC_FLUSH_CONTEXT => "flush_context",
        cmd::CC_START_AUTH_SESSION => "start_auth_session",
        cmd::CC_POLICY_PCR => "policy_pcr",
        cmd::CC_POLICY_AUTHORIZE => "policy_authorize",
        cmd::CC_POLICY_GET_DIGEST => "policy_get_digest",
        cmd::CC_PCR_READ => "pcr_read",
        cmd::CC_PCR_EXTEND => "pcr_extend",
        cmd::CC_GET_CAPABILITY => "get_capability",
        other => panic!("no seed for the command {other:#x}"),
    }
}

/// A seed of `tpm_response`: the index of `code`, then `response`.
fn framed(code: u32, response: &[u8]) -> Vec<u8> {
    let index = COMMANDS.iter().position(|c| *c == code).expect("one of the fourteen commands");
    [&[index as u8][..], response].concat()
}

/// A vector of `crates/pod-tpm/tests/testdata/` (a Base64 body) as PEM.
fn vector(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/pod-tpm/tests/testdata").join(file);
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    format!("{BEGIN}\n{}\n{END}\n", body.trim()).into_bytes()
}

/// The target, the name and the bytes of each seed.
fn seeds() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let (seen, sealed, signed) = run();
    let mut out: Vec<(&str, &str, Vec<u8>)> = Vec::new();
    for (code, response) in &seen {
        if response[6..10] == [0; 4] && !out.iter().any(|(_, n, _)| *n == name(*code)) {
            out.push(("tpm_response", name(*code), framed(*code, response)));
        }
    }
    let refused = seen.iter().find(|(_, r)| r[6..10] != [0; 4]).expect("a refusal");
    out.push(("tpm_response", "refused", framed(refused.0, &refused.1)));
    let mut capabilities = seen.iter().filter(|(c, _)| *c == cmd::CC_GET_CAPABILITY);
    out.push((
        "tpm_response",
        "get_capability_algorithms",
        framed(cmd::CC_GET_CAPABILITY, &capabilities.nth(2).unwrap().1),
    ));
    out.push((
        "tpm_response",
        "get_capability_handles",
        framed(cmd::CC_GET_CAPABILITY, &capabilities.next_back().unwrap().1),
    ));
    out.push(("tpm_keyfile", "pcr_policy_der", sealed.to_der().unwrap()));
    out.push(("tpm_keyfile", "signed_policy_pem", signed.to_pem().unwrap().into_bytes()));
    out.push(("tpm_keyfile", "go_tpm_keyfiles_pem", vector("skey.b64")));
    out.push(("tpm_keyfile", "openssl_tpm2_engine_pem", vector("p256-authvalue.b64")));
    out
}

fn dir(target: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join(target)
}

/// The seeds on disk are the ones of [`seeds`]: a response of each of the
/// fourteen commands, a refusal and the key files; each reads, and the
/// target takes each.
#[test]
fn the_seeds_of_the_tpm_targets_are_the_responses_of_the_tpm_in_software() {
    let seeds = seeds();
    for (target, name, bytes) in &seeds {
        let on_disk = std::fs::read(dir(target).join(name)).unwrap_or_else(|e| panic!("{target}/{name}: {e}"));
        assert!(on_disk == *bytes, "{target}/{name} is stale: run write_the_tpm_seeds");
    }
    for target in ["tpm_response", "tpm_keyfile"] {
        let mut on_disk: Vec<String> = std::fs::read_dir(dir(target))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();
        let mut made: Vec<String> = seeds.iter().filter(|s| s.0 == target).map(|s| s.1.to_string()).collect();
        made.sort();
        assert_eq!(on_disk, made, "{target}: a seed that this file does not make");
    }
    let responses: Vec<_> = seeds.iter().filter(|s| s.0 == "tpm_response").collect();
    assert_eq!(responses.len(), COMMANDS.len() + 3, "a response of each command, a refusal, two capabilities");
    for (target, name, bytes) in &seeds {
        if *target == "tpm_response" {
            let code = COMMANDS[usize::from(bytes[0])];
            let read = pod_tpm::Reply::read(code, &bytes[1..]);
            assert_eq!(read.is_err(), *name == "refused", "{name}: {:?}", read.err());
            crate::targets::tpm_response(bytes);
        } else {
            KeyFile::read(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            crate::targets::tpm_keyfile(bytes);
        }
    }
}

#[test]
#[ignore = "writes the seeds of tpm_response and tpm_keyfile into fuzz/seeds/"]
fn write_the_tpm_seeds() {
    for (target, name, bytes) in seeds() {
        std::fs::create_dir_all(dir(target)).unwrap();
        std::fs::write(dir(target).join(name), bytes).unwrap();
    }
}
