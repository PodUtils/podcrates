//! The commands on byte vectors, each field where part 3 puts it, written
//! out by hand from the specification and not from the encoder; and the
//! readers of their responses.

mod common;

use common::unhex;
use pod_tpm::cmd::{self, SessionKind, Ticket};
use pod_tpm::public::{self, NULL_SIGNATURE};
use pod_tpm::wire::{Auth, RH_OWNER};
use pod_tpm::{EccPublic, EcdsaSignature, Error, HashAlg, Reply};

/// The name of a command, the bytes of the crate, and the bytes by hand.
type Command<'a> = (&'a str, Result<Vec<u8>, Error>, String);

fn check(cases: Vec<Command>) {
    for (name, made, want) in cases {
        let made = made.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(made, unhex(&want), "{name}");
    }
}

/// The policy digest of PCR 7 of zeros (`the_digest_of_a_policy_of_pcrs_is_part_1s`).
const PCR7_ZERO_POLICY: &str = "8b5682d81b29435d08d79278150611dc7e5923b2fefcce684a09577b40130a8b";

/// podssh's vectors of the commands, and those of the commands that it
/// did not write out.
#[test]
fn the_commands_are_the_bytes_of_part_3() {
    let (n32, n16, d, e) = ("11".repeat(32), "22".repeat(16), "33".repeat(32), "44".repeat(32));
    let signing = format!("0038 0023 000b 000404b2 0020 {PCR7_ZERO_POLICY} 0010 0018 000b 0003 0010 0000 0000");
    check(vec![
        ("FlushContext", cmd::flush_context(0x8000_0001), "8001 0000000e 00000165 80000001".into()),
        (
            "CreatePrimary of the H-2 storage key",
            public::storage_primary().and_then(|t| cmd::create_primary(RH_OWNER, &t)),
            "8002 00000043 00000131 40000001 00000009 40000009 0000 00 0000 \
             0004 0000 0000 001a 0023 000b 00030472 0000 0006 0080 0043 0010 0003 0010 0000 0000 \
             0000 00000000"
                .into(),
        ),
        (
            "the template of a key sealed to PCR 7 of zeros",
            public::signing_key(&unhex(PCR7_ZERO_POLICY)),
            signing.clone(),
        ),
        (
            "Create of that key under a transient parent",
            public::signing_key(&unhex(PCR7_ZERO_POLICY)).and_then(|t| cmd::create(0x8000_0000, &t)),
            format!(
                "8002 00000061 00000153 80000000 00000009 40000009 0000 00 0000 0004 0000 0000 {signing} 0000 00000000"
            ),
        ),
        (
            "Load under the persistent storage key",
            cmd::load(0x8100_0001, &unhex("0004 deadbeef"), &unhex("0002 abcd")),
            "8002 00000025 00000157 81000001 00000009 40000009 0000 00 0000 0004deadbeef 0002abcd".into(),
        ),
        (
            "PCR_Read of 0,7,11",
            cmd::pcr_read(&unhex("00000001 000b 03 810800")),
            "8001 00000014 0000017e 00000001 000b 03 810800".into(),
        ),
        (
            "PCR_Extend of PCR 7 in the SHA-256 bank",
            cmd::pcr_extend(7, &[0x44; 32]),
            format!("8002 00000041 00000182 00000007 00000009 40000009 0000 00 0000 00000001 000b {e}"),
        ),
        (
            "StartAuthSession of a policy session of SHA-256",
            cmd::start_auth_session(SessionKind::Policy, &[0x11; 32], HashAlg::Sha256),
            format!("8001 0000003b 00000176 40000007 40000007 0020 {n32} 0000 01 0010 000b"),
        ),
        (
            "PolicyPCR of PCR 7 with the digest of its values",
            cmd::policy_pcr(0x0300_0000, &[0x33; 32], &unhex("00000001 000b 03 800000")),
            format!("8001 0000003a 0000017f 03000000 0020 {d} 00000001 000b 03 800000"),
        ),
        ("PolicyGetDigest", cmd::policy_get_digest(0x0300_0001), "8001 0000000e 00000189 03000001".into()),
        (
            "Sign of a SHA-256 digest with ECDSA in a policy session",
            cmd::sign(0x8000_0001, &Auth::policy(0x0300_0000, &[0x22; 16]), &[0x33; 32]),
            format!(
                "8002 00000059 0000015d 80000001 00000019 03000000 0010 {n16} 00 0000 \
                 0020 {d} 0018 000b 8024 40000007 0000"
            ),
        ),
        (
            "GetCapability of the manufacturer",
            cmd::get_manufacturer(),
            "8001 00000016 0000017a 00000006 00000105 00000001".into(),
        ),
    ]);
}

/// The commands of the offline update by a policy key: the public area of
/// the policy key, `PolicyAuthorize` in a trial with the ticket of
/// nothing, `LoadExternal` in the owner hierarchy, `VerifySignature`, and
/// the reader of the ticket.
#[test]
fn the_parts_of_a_signed_policy_are_the_bytes_of_part_3() {
    let (x, y, r, d) = ("11".repeat(32), "22".repeat(32), "33".repeat(32), "44".repeat(32));
    let area = format!("0056 0023 000b 00060040 0000 0010 0010 0003 0010 0020 {x} 0020 {y}");
    let public = public::external_p256(&unhex(&format!("04 {x} {y}"))).unwrap();
    assert_eq!(public, unhex(&area));
    let named = "000b5b685f1846a972347b5dd1c29006510804caf58a5981ab0b44288538c20f4b17";
    check(vec![
        (
            "PolicyAuthorize in a trial, with the ticket of nothing",
            cmd::policy_authorize(0x0300_0000, &[], &unhex(&r), &unhex(named), &Ticket::null()),
            format!("8001 0000005e 0000016a 03000000 0000 0020 {r} 0022 {named} 8022 40000007 0000"),
        ),
        (
            "LoadExternal of the public area alone, in the owner's hierarchy",
            cmd::load_external(&public, RH_OWNER),
            format!("8001 00000068 00000167 0000 {area} 40000001"),
        ),
        (
            "VerifySignature",
            cmd::verify_signature(0x8000_0002, &unhex(&d), NULL_SIGNATURE),
            format!("8001 00000032 00000177 80000002 0020 {d} 0010"),
        ),
    ]);
    let reply = |params: &str| Reply { handles: vec![], params: unhex(params) };
    let ticket = Ticket::read(&reply("8022 40000001 0002 abcd")).unwrap();
    assert_eq!(ticket, Ticket(unhex("8022 40000001 0002 abcd")));
    assert!(Ticket::read(&reply("8022 40000001 0002 abcd 99")).is_err(), "a byte after the ticket");
    assert!(Ticket::read(&reply("8021 40000001 0000")).is_err(), "another tag");
    for bad in ["04 11", "03 0000"] {
        assert!(public::external_p256(&unhex(bad)).is_err(), "{bad}");
    }
}

/// The signature of `TPM2_Sign`: `r` and `s` padded to 32 bytes, and back
/// to a `TPMT_SIGNATURE`; another algorithm, another hash, or a zero `r`,
/// is refused.
#[test]
fn a_signature_of_the_tpm_gives_r_and_s() {
    let reply = |params: Vec<u8>| Reply { handles: vec![], params };
    let mut tpmt = unhex("0018 000b 0020");
    tpmt.extend_from_slice(&[0x80; 32]);
    tpmt.extend_from_slice(&unhex("001f"));
    tpmt.extend_from_slice(&[0x01; 31]);
    let signature = EcdsaSignature::read(&reply(tpmt)).unwrap();
    assert_eq!(signature.r, [0x80; 32]);
    let mut s = [0x01; 32];
    s[0] = 0;
    assert_eq!(signature.s, s);
    let back = signature.to_tpmt();
    assert_eq!(EcdsaSignature::read(&reply(back.clone())).unwrap(), signature);
    assert_eq!(back.len(), 4 + 2 * 34);
    for (bad, why) in [
        ("0014 000b 0000 0000", "RSASSA"),
        ("0018 0004 0000 0000", "SHA-1"),
        ("0018 000b 0000 0001 01", "an r of zero"),
        ("0018 000b 0001 01 0000", "an s of zero"),
    ] {
        assert!(EcdsaSignature::read(&reply(unhex(bad))).is_err(), "{why}");
    }
    let mut long = unhex("0018 000b 0021");
    long.extend_from_slice(&[0x01; 33]);
    long.extend_from_slice(&unhex("0001 01"));
    assert!(EcdsaSignature::read(&reply(long)).is_err(), "an r of 33 bytes");
}

/// The readers of the responses, each by hand from part 3.
#[test]
fn the_responses_are_read_as_part_3_gives_them() {
    let read = |code: u32, text: &str| Reply::read(code, &unhex(text)).unwrap();
    let primary = cmd::Primary::read(&read(
        cmd::CC_CREATE_PRIMARY,
        "8002 0000002d 00000000 80000001 00000016 0004 0023 000b 0000 0000 8021 40000001 0000 0002 000b 0000 01 0000",
    ))
    .unwrap();
    assert_eq!(primary, cmd::Primary { handle: 0x8000_0001, public: unhex("0004 0023 000b"), name: unhex("000b") });
    let created = cmd::Created::read(&read(
        cmd::CC_CREATE,
        "8002 00000027 00000000 00000014 0002 aaaa 0002 bbbb 0000 0000 8021 40000001 0000 0000 01 0000",
    ))
    .unwrap();
    assert_eq!(created, cmd::Created { private: unhex("0002 aaaa"), public: unhex("0002 bbbb") });
    let started = cmd::Started::read(&read(
        cmd::CC_START_AUTH_SESSION,
        &format!("8001 00000020 00000000 03000000 0010 {}", "77".repeat(16)),
    ))
    .unwrap();
    assert_eq!(started, cmd::Started { handle: 0x0300_0000, nonce_tpm: vec![0x77; 16] });
    let digest = cmd::policy_digest(&read(
        cmd::CC_POLICY_GET_DIGEST,
        &format!("8001 0000002c 00000000 0020 {PCR7_ZERO_POLICY}"),
    ))
    .unwrap();
    assert_eq!(digest, unhex(PCR7_ZERO_POLICY));
    let values = cmd::PcrValues::read(&read(
        cmd::CC_PCR_READ,
        &format!("8001 0000003e 00000000 00000007 00000001 000b 03 800000 00000001 0020 {}", "00".repeat(32)),
    ))
    .unwrap();
    assert_eq!(values.counter, 7);
    assert_eq!(values.selection, unhex("00000001 000b 03 800000"));
    assert_eq!(values.values, vec![vec![0u8; 32]]);
    let capability = cmd::Capability::read(&read(
        cmd::CC_GET_CAPABILITY,
        "8001 0000001b 00000000 00 00000006 00000001 00000105 49465800",
    ))
    .unwrap();
    assert_eq!(capability.manufacturer().as_deref(), Some("IFX"));
    assert!(!capability.more);
    let handles = cmd::Capability::read(&read(
        cmd::CC_GET_CAPABILITY,
        "8001 0000001b 00000000 01 00000001 00000002 80000000 80000001",
    ))
    .unwrap();
    assert_eq!(handles.data, cmd::CapabilityData::Handles(vec![0x8000_0000, 0x8000_0001]));
    assert!(handles.more);
    let empty = read(cmd::CC_FLUSH_CONTEXT, "8001 0000000a 00000000");
    assert!(empty.empty().is_ok());
    assert!(Reply { handles: vec![], params: vec![0] }.empty().is_err());
}

/// The public area of a key: an ECC P-256 key is read; a key of another
/// type or on another curve is refused by its name.
#[test]
fn a_public_area_of_another_type_or_curve_is_refused_by_name() {
    let (x, y) = ("00".to_string() + &"11".repeat(31), "22".repeat(32));
    let area = public::external_p256(&unhex(&format!("04 {x} {y}"))).unwrap();
    let key = EccPublic::read(&area).unwrap();
    assert_eq!(key.name_alg, 0x000B);
    assert_eq!(key.attributes, public::EXTERNAL);
    assert_eq!(&key.point[..], &unhex(&format!("04 {x} {y}"))[..]);
    // The TPM cuts a leading zero of a coordinate: the point is the same.
    let short = format!("0055 0023 000b 00060040 0000 0010 0010 0003 0010 001f {} 0020 {y}", "11".repeat(31));
    assert_eq!(EccPublic::read(&unhex(&short)).unwrap().point, key.point);
    let rsa = unhex("0008 0001 000b 00000000");
    let e = EccPublic::read(&rsa).unwrap_err();
    assert!(matches!(e, Error::Unsupported(_)) && e.to_string().contains("RSA (0x0001)"), "{e}");
    let p384 = "0016 0023 000b 00060040 0000 0010 0010 0004 0010 0000 0000";
    let e = EccPublic::read(&unhex(p384)).unwrap_err();
    assert!(e.to_string().contains("curve 0x0004"), "{e}");
    let template = public::signing_key(&unhex(PCR7_ZERO_POLICY)).unwrap();
    let key = EccPublic::read(&template).unwrap();
    assert_eq!(key.auth_policy, unhex(PCR7_ZERO_POLICY));
    assert_eq!(key.attributes, public::SIGNING);
    assert!(key.attributes & public::USER_WITH_AUTH == 0, "no password for a key under a policy");
    assert!(public::signing_key(&[0; 65]).is_err(), "an authPolicy longer than a digest");
}

/// A value of the caller that cannot go into a command is refused before
/// a byte is sent: a nonce out of range, a PCR past 31, a command over
/// the bound.
#[test]
fn a_bad_value_of_the_caller_is_refused() {
    for nonce in [15, 33] {
        let e = cmd::start_auth_session(SessionKind::Policy, &vec![0; nonce], HashAlg::Sha256).unwrap_err();
        assert!(matches!(e, Error::Usage(_)), "{nonce}: {e}");
    }
    assert!(cmd::start_auth_session(SessionKind::Trial, &[0; 48], HashAlg::Sha384).is_ok());
    assert!(matches!(cmd::pcr_extend(32, &[0; 32]), Err(Error::Usage(_))));
    let big = vec![0u8; 4096];
    let e = cmd::load(0x8100_0001, &big, &[]).unwrap_err();
    assert!(e.to_string().contains("longer than the 4096"), "{e}");
    let huge = vec![0u8; 70_000];
    assert!(cmd::verify_signature(0x8000_0000, &huge, NULL_SIGNATURE).is_err(), "a TPM2B over its length field");
}
