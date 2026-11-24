//! The key file `TSS2 PRIVATE KEY` (draft-bottomley-tpm-keys-00): the
//! files of another program read and written back to the same bytes, a
//! loadable key under a policy of PCRs and under signed policies in DER
//! and in PEM, and the layout of a signed policy as the draft gives it.

mod common;

use common::{tlv, unhex};
use pod_tpm::cmd::{CC_POLICY_AUTH_VALUE, CC_POLICY_AUTHORIZE, CC_POLICY_PCR};
use pod_tpm::keyfile::{self, AuthPolicy, KeyFile, Kind};
use pod_tpm::public::{self, NULL_SIGNATURE};
use pod_tpm::wire::RH_OWNER;
use pod_tpm::{Authorize, EccPublic, EcdsaSignature, HashAlg, PcrSelection, Policy, Statement, policy};

/// A key file of go-tpm-keyfiles' tests (Foxboron/go-tpm-keyfiles,
/// `testdata/p256-authvalue.tpm` at `8c9c2d10`, MIT), as podssh keeps it: a
/// P-256 key of a SHA-384 name, whose policy is a PolicyPCR of PCR 16 and a
/// PolicyAuthValue. Its body is kept without the PEM lines, which the scan
/// for private keys would read as a credential.
const FOREIGN_BODY: &str = include_str!("testdata/p256-authvalue.b64");

/// A key file of go-tpm-keyfiles' tests (`testdata/skey.tpm` at
/// `8c9c2d10`, MIT): an RSA key whose policy is a PolicyAuthorize of a P-256
/// policy key, with one signed policy named `secret` (a PolicySecret, then
/// the signed PolicyAuthorize).
const SIGNED_BODY: &str = include_str!("testdata/skey.b64");

/// The PEM text of a body of 64 characters a line.
fn pem(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    format!("-----BEGIN TSS2 PRIVATE KEY-----\n{body}-----END TSS2 PRIVATE KEY-----\n")
}

/// The policy key of the tests: a made-up point of P-256.
fn policy_key() -> Vec<u8> {
    let point = unhex(&format!("04 {} {}", "11".repeat(32), "22".repeat(32)));
    public::external_p256(&point).unwrap()
}

/// A key under a PolicyPCR of PCR 7 of zeros, as the crate makes it.
fn pcr_key(parent: u32) -> KeyFile {
    let selection = PcrSelection::parse("7").unwrap();
    let values = policy::pcr_values_digest(HashAlg::Sha256, &[&[0; 32]]).unwrap();
    let policy = Policy::new(vec![Statement::pcr(&selection, &values)]);
    let template = public::signing_key(&policy.digest(HashAlg::Sha256).unwrap()).unwrap();
    let mut file = KeyFile::loadable(parent, template, unhex("0006 0102 0304 0506"));
    file.policy = policy;
    file.description = Some("me@lab".into());
    file
}

/// A key under a policy key, with two signed policies of PCR 7.
fn signed_key() -> KeyFile {
    let policy_ref = vec![0x33; 32];
    let head = Authorize { public: policy_key(), policy_ref: policy_ref.clone(), signature: NULL_SIGNATURE.to_vec() };
    let own = Policy::new(vec![Statement::authorize(&head).unwrap()]);
    let template = public::signing_key(&own.digest(HashAlg::Sha256).unwrap()).unwrap();
    let mut file = KeyFile::loadable(0x8100_0001, template, unhex("0004 a1a2a3a4"));
    file.policy = own;
    let selection = PcrSelection::parse("7").unwrap();
    for (n, name) in [(1u8, Some("boot 2".to_string())), (2, None)] {
        let values = policy::pcr_values_digest(HashAlg::Sha256, &[&[n; 32]]).unwrap();
        let signature = EcdsaSignature { r: [n; 32], s: [n + 0x10; 32] }.to_tpmt();
        let signed = Authorize { public: policy_key(), policy_ref: policy_ref.clone(), signature };
        let steps = vec![Statement::pcr(&selection, &values), Statement::authorize(&signed).unwrap()];
        file.auth_policy.push(AuthPolicy { name, policy: Policy::new(steps) });
    }
    file
}

/// Read back the DER and the PEM, and the bytes of each are the bytes of
/// the first write.
fn round_trip(file: &KeyFile) {
    let der = file.to_der().unwrap();
    let again = KeyFile::from_der(&der).unwrap();
    assert_eq!(&again, file);
    assert_eq!(again.to_der().unwrap(), der, "DER has one form");
    let text = file.to_pem().unwrap();
    assert!(text.starts_with("-----BEGIN TSS2 PRIVATE KEY-----\n"), "{text}");
    assert!(text.ends_with("-----END TSS2 PRIVATE KEY-----\n"), "{text}");
    assert!(text.lines().all(|l| l.len() <= 64), "{text}");
    assert!(keyfile::is_pem(text.as_bytes()));
    assert_eq!(&KeyFile::from_pem(&text).unwrap(), file);
    assert_eq!(&KeyFile::read(text.as_bytes()).unwrap(), file);
    assert_eq!(&KeyFile::read(&der).unwrap(), file);
}

/// The key file of another program: read, its policy digest computed
/// again to the key's `authPolicy`, its public key read, and written back
/// to the same bytes.
#[test]
fn a_key_file_of_go_tpm_keyfiles_is_read_and_written_back() {
    let foreign = pem(FOREIGN_BODY);
    assert!(keyfile::is_pem(foreign.as_bytes()));
    let file = KeyFile::from_pem(&foreign).unwrap();
    assert_eq!(file.kind, Kind::Loadable);
    assert!(file.empty_auth && file.auth_policy.is_empty() && !file.rsa_parent);
    assert_eq!(file.parent, RH_OWNER);
    let codes: Vec<u32> = file.policy.statements.iter().map(|s| s.code).collect();
    assert_eq!(codes, vec![CC_POLICY_PCR, CC_POLICY_AUTH_VALUE]);
    let area = EccPublic::read(&file.public).unwrap();
    assert_eq!(area.name_alg, HashAlg::Sha384.id());
    assert_eq!(file.policy.digest(HashAlg::Sha384).unwrap(), area.auth_policy);
    let (selection, _) = file.policy.statements[0].read_pcr().unwrap();
    assert_eq!(selection.to_string(), "sha256:16");
    assert_eq!(file.to_pem().unwrap(), foreign);
}

/// A key file with a signed policy of go-tpm-keyfiles: the key's own
/// policy is the PolicyAuthorize of the policy key, and its signed policy
/// ends with a PolicyAuthorize of the same key. It is written back to the
/// same bytes.
#[test]
fn a_key_file_with_signed_policies_of_go_tpm_keyfiles_reads_back() {
    let text = pem(SIGNED_BODY);
    let file = KeyFile::from_pem(&text).unwrap();
    assert!(!file.empty_auth && !file.rsa_parent && file.description.is_none());
    assert_eq!(file.parent, RH_OWNER);
    assert_eq!(file.policy.statements.len(), 1);
    assert_eq!(file.policy.statements[0].code, CC_POLICY_AUTHORIZE);
    let head = Authorize::parse(&file.policy.statements[0].body).unwrap();
    assert_eq!(head.signature, NULL_SIGNATURE);
    assert_eq!(file.auth_policy.len(), 1);
    let signed = &file.auth_policy[0];
    assert_eq!(signed.name.as_deref(), Some("secret"));
    let codes: Vec<u32> = signed.policy.statements.iter().map(|s| s.code).collect();
    assert_eq!(codes, vec![0x0000_0151, CC_POLICY_AUTHORIZE], "PolicySecret, then PolicyAuthorize");
    let last = Authorize::parse(&signed.policy.statements[1].body).unwrap();
    assert_eq!((&last.public, &last.policy_ref), (&head.public, &head.policy_ref), "the same policy key");
    assert_ne!(last.signature, NULL_SIGNATURE);
    assert_eq!(file.to_pem().unwrap(), text);
}

/// A key file with signed policies, its DER written out by hand from the
/// draft: a TPMAuthPolicy of a name and two commands, after the key's own
/// policy. One with no command is refused, as go-tpm-keyfiles refuses it.
#[test]
fn signed_policies_are_written_as_the_draft_says() {
    let statement = |code, body: &[u8]| Statement { code, body: body.to_vec() };
    let signed = AuthPolicy {
        name: Some("a".into()),
        policy: Policy::new(vec![statement(CC_POLICY_PCR, &[0xBB]), statement(CC_POLICY_AUTHORIZE, &[0xCC])]),
    };
    let mut file = KeyFile::loadable(RH_OWNER, vec![0, 0], vec![0, 0]);
    file.policy = Policy::new(vec![statement(CC_POLICY_AUTHORIZE, &[0xAA])]);
    file.auth_policy = vec![signed];
    let want = unhex(
        "3055 06066781050a0103 a0030101ff a10f300d300ba0040202016aa1030401aa \
         a327 3025 3023 a0030c0161 a11c301a 300ba0040202017fa1030401bb 300ba0040202016aa1030401cc \
         020440000001 04020000 04020000",
    );
    assert_eq!(file.to_der().unwrap(), want);
    assert_eq!(KeyFile::from_der(&want).unwrap(), file);
    let empty = unhex("3025 06066781050a0103 a0030101ff a308300630 04a1023000 020440000001 04020000 04020000");
    assert!(KeyFile::from_der(&empty).unwrap_err().to_string().contains("authPolicy"));
}

/// A loadable key under a policy of PCRs, under the owner's primary and
/// under the persistent storage key, in DER and in PEM.
#[test]
fn a_loadable_key_under_a_pcr_policy_round_trips() {
    for parent in [RH_OWNER, 0x8100_0001] {
        let file = pcr_key(parent);
        round_trip(&file);
        let area = EccPublic::read(&file.public).unwrap();
        assert_eq!(file.policy.digest(HashAlg::Sha256).unwrap(), area.auth_policy);
    }
    let mut rsa = pcr_key(RH_OWNER);
    rsa.rsa_parent = true;
    round_trip(&rsa);
    let mut quiet = pcr_key(RH_OWNER);
    quiet.empty_auth = false;
    quiet.description = None;
    round_trip(&quiet);
    // No emptyAuth when false: the draft's advice to a writer.
    assert!(!quiet.to_der().unwrap().windows(5).any(|w| w == [0xA0, 0x03, 0x01, 0x01, 0xFF]));
}

/// A loadable key under signed policies: each signed policy keeps its name,
/// its statements and the bytes of its signature; its approved policy is
/// the digest before its PolicyAuthorize.
#[test]
fn a_loadable_key_under_signed_policies_round_trips() {
    let file = signed_key();
    round_trip(&file);
    let area = EccPublic::read(&file.public).unwrap();
    assert_eq!(file.policy.digest(HashAlg::Sha256).unwrap(), area.auth_policy);
    let first = &file.auth_policy[0].policy;
    let values = policy::pcr_values_digest(HashAlg::Sha256, &[&[1; 32]]).unwrap();
    let want = policy::policy_pcr_update(HashAlg::Sha256, &[0; 32], &PcrSelection::parse("7").unwrap(), &values);
    assert_eq!(first.approved(HashAlg::Sha256).unwrap(), want.unwrap());
    assert_eq!(file.auth_policy[1].name, None);
}

/// An old file of openssl_tpm2_engine names the loadable key with the OID
/// 2.23.133.10.2: it is read as a loadable key, and written with the OID of
/// the draft.
#[test]
fn the_old_oid_of_a_loadable_key_is_read() {
    let old = [0x06, 0x05, 0x67, 0x81, 0x05, 0x0A, 0x02];
    let body = [&old[..], &common::der_int(RH_OWNER), &tlv(0x04, &[0, 0]), &tlv(0x04, &[0, 0])].concat();
    let file = KeyFile::from_der(&tlv(0x30, &body)).unwrap();
    assert_eq!(file.kind, Kind::Loadable);
    assert!(file.to_der().unwrap().starts_with(&[0x30, 0x16, 0x06, 0x06, 0x67, 0x81, 0x05, 0x0A, 0x01, 0x03]));
}
