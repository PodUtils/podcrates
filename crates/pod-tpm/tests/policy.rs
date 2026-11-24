//! The PCR selection and the policy digests. Each vector says its source: a
//! vector of podssh's module, or a computation by hand from part 3 with
//! Python's hashlib. The digest that swtpm gives with `PolicyGetDigest` is
//! the proof of T-1222.

mod common;

use common::unhex;
use pod_tpm::cmd::{CC_POLICY_AUTH_VALUE, CC_POLICY_AUTHORIZE, CC_POLICY_PASSWORD, CC_POLICY_PCR};
use pod_tpm::policy::{self, approved_policy_hash, policy_authorize_digest, policy_pcr_digest};
use pod_tpm::{Authorize, Error, HashAlg, PcrSelection, Policy, Statement, public};

/// The digest of a policy of PCR 7 of zeros (podssh's vector).
const PCR7_ZERO: &str = "8b5682d81b29435d08d79278150611dc7e5923b2fefcce684a09577b40130a8b";
/// The name of the policy key of x = 0x11..., y = 0x22... (podssh's vector).
const NAME: &str = "000b5b685f1846a972347b5dd1c29006510804caf58a5981ab0b44288538c20f4b17";

fn sel(text: &str) -> PcrSelection {
    PcrSelection::parse(text).unwrap()
}

/// The PCRs as a user names them, and their selection (podssh's test; a
/// selection now holds each bank, and `only` refuses another bank with
/// bits set, as podssh's reader did).
#[test]
fn the_pcrs_are_named_and_selected() {
    for (text, numbers) in [("7", vec![7]), ("0,7,11", vec![0, 7, 11]), (" 23 , 0 ", vec![0, 23]), ("7,7", vec![7])] {
        assert_eq!(sel(text).pcrs(HashAlg::Sha256), numbers, "{text}");
    }
    for bad in ["", "24", "-1", "a", "7,", "1.5", "md5:1", "sha256:"] {
        assert!(PcrSelection::parse(bad).is_err(), "{bad:?}");
    }
    let pcrs = sel("0,7,11");
    assert_eq!(pcrs.to_string(), "sha256:0,7,11");
    assert_eq!(PcrSelection::from_bytes(&unhex("00000001 000b 03 810800")).unwrap(), pcrs);
    // Another bank with bits set is refused by `only`; one with none is
    // passed over.
    let sha1 = PcrSelection::from_bytes(&unhex("00000001 0004 03 800000")).unwrap();
    assert!(matches!(sha1.only(HashAlg::Sha256), Err(Error::Unsupported(_))));
    let both = PcrSelection::from_bytes(&unhex("00000002 0004 03 000000 000b 03 800000")).unwrap();
    assert_eq!(both.only(HashAlg::Sha256).unwrap(), vec![7]);
    let two = sel("sha256:0,7+sha1:1");
    assert_eq!(two.to_string(), "sha256:0,7+sha1:1");
    assert_eq!(two.marshal(), unhex("00000002 000b 03 810000 0004 03 020000"));
    assert_eq!(two.count(), 3);
}

/// Each PCR alone: its bit, its bytes, and a digest of its own.
#[test]
fn a_selection_of_each_pcr_has_its_bit_and_its_digest() {
    let mut digests = Vec::new();
    for pcr in 0..24u32 {
        let one = PcrSelection::of(HashAlg::Sha256, &[pcr]).unwrap();
        let mut bits = [0u8; 3];
        bits[(pcr / 8) as usize] = 1 << (pcr % 8);
        let mut want = unhex("00000001 000b 03");
        want.extend_from_slice(&bits);
        assert_eq!(one.marshal(), want, "PCR {pcr}");
        assert_eq!(PcrSelection::from_bytes(&want).unwrap(), one);
        digests.push(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &one, &[&[0; 32]]).unwrap());
    }
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), 24, "a digest for each PCR");
    assert!(PcrSelection::of(HashAlg::Sha256, &[24]).is_err());
}

/// A selection out of its bounds is refused: a count of banks over 16, a
/// bit set of more than 4 octets, a PCR from 24 up, a byte after it. A bit
/// set of 4 octets is read and written back as it came.
#[test]
fn a_selection_past_its_bounds_is_refused() {
    let seventeen = unhex(&format!("00000011 {}", "000b 03 800000 ".repeat(17)));
    assert!(PcrSelection::from_bytes(&seventeen).unwrap_err().to_string().contains("17 PCR banks"));
    let five = unhex("00000001 000b 05 8000000000");
    assert!(PcrSelection::from_bytes(&five).unwrap_err().to_string().contains("5 octets"));
    let high = unhex("00000001 000b 04 00000001");
    assert!(PcrSelection::from_bytes(&high).unwrap_err().to_string().contains("PCR 24 and up"));
    assert!(PcrSelection::from_bytes(&unhex("00000001 000b 03 800000 00")).is_err());
    assert!(PcrSelection::from_bytes(&unhex("00000002 000b 03 800000")).is_err(), "a bank due and none came");
    let four = unhex("00000001 000b 04 80000000");
    let read = PcrSelection::from_bytes(&four).unwrap();
    assert_eq!(read.marshal(), four);
    assert_eq!(read.pcrs(HashAlg::Sha256), vec![7]);
}

/// The policy digest of PCR 7 holding zeros, from part 1's rule outside
/// the crate: H(zeros || TPM_CC_PolicyPCR || selection || H(values))
/// (podssh's test and vector).
#[test]
fn the_digest_of_a_policy_of_pcrs_is_part_1s() {
    let zeros = policy::pcr_values_digest(HashAlg::Sha256, &[&[0u8; 32]]).unwrap();
    let statement = Statement::pcr(&sel("7"), &zeros);
    assert_eq!(statement.code, CC_POLICY_PCR);
    let body = unhex("00000001000b03800000 66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925");
    assert_eq!(statement.body, body);
    let digest = Policy::new(vec![statement.clone()]).digest(HashAlg::Sha256).unwrap();
    assert_eq!(digest, unhex(PCR7_ZERO));
    assert_eq!(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &sel("7"), &[&[0; 32]]).unwrap(), digest);
    // The order of the command (the digest as a TPM2B first) reads the same.
    let mut tpm_order = vec![0x00, 0x20];
    tpm_order.extend_from_slice(&body[10..]);
    tpm_order.extend_from_slice(&body[..10]);
    let other = Statement { code: CC_POLICY_PCR, body: tpm_order };
    assert_eq!(Policy::new(vec![other.clone()]).digest(HashAlg::Sha256).unwrap(), digest);
    assert_eq!(other.read_pcr().unwrap(), statement.read_pcr().unwrap());
    let unknown = Policy::new(vec![Statement { code: 0x0000_0170, body: vec![] }]).digest(HashAlg::Sha256);
    assert!(matches!(unknown, Err(Error::Unsupported(_))), "a command that the crate does not compute");
}

/// More digests of PolicyPCR, each by hand from part 3, 23.7, with
/// Python's hashlib: a session of SHA-384; two banks; a second PolicyPCR
/// after the first.
#[test]
fn the_digest_of_policy_pcr_is_part_3s_in_each_case() {
    let sha384 = policy_pcr_digest(HashAlg::Sha384, &[0; 48], &sel("7"), &[&[0; 32]]).unwrap();
    assert_eq!(
        sha384,
        unhex("189cea1aa37436317828e1b2583a7e844874802720bce034cd034c76b4fa45c26a10ab1f31db6d0a8fee7f31af2eead9")
    );
    let two = policy_pcr_digest(HashAlg::Sha256, &[0; 32], &sel("sha1:0+sha256:7"), &[&[0; 20], &[0; 32]]).unwrap();
    assert_eq!(two, unhex("245d32bf01cb04bc349d190b6fd01d0ae6455b712867bc94f98d6aebd2cd0e84"));
    let values: [&[u8]; 3] = [&[1; 32], &[2; 32], &[3; 32]];
    let chained = policy_pcr_digest(HashAlg::Sha256, &unhex(PCR7_ZERO), &sel("0,7,11"), &values).unwrap();
    assert_eq!(chained, unhex("dbe9b63948a053463e85f14dc795a8257208de61c87e4c06a2dcd92b67b8e911"));
}

/// A list of values that does not match the selection is refused: a count,
/// a size of the bank, an old digest of another size, an empty bank, no
/// bank, a session of SHA-1.
#[test]
fn a_digest_of_values_that_do_not_match_is_refused() {
    let usage = |r: Result<Vec<u8>, Error>| matches!(r, Err(Error::Usage(_)));
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &sel("0,7"), &[&[0; 32]])), "one value for two");
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &sel("7"), &[&[0; 32], &[0; 32]])), "two for one");
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &sel("7"), &[&[0; 20]])), "a value of SHA-1");
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 48], &sel("7"), &[&[0; 32]])), "an old digest of SHA-384");
    let empty = PcrSelection::from_bytes(&unhex("00000002 0004 03 000000 000b 03 800000")).unwrap();
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &empty, &[&[0; 32]])), "an empty bank");
    assert!(usage(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &PcrSelection::new(), &[])), "no bank");
    let sha1 = policy_pcr_digest(HashAlg::Sha1, &[0; 20], &sel("7"), &[&[0; 32]]);
    assert!(matches!(sha1, Err(Error::Unsupported(_))), "a session of SHA-1");
    let empty_digest = Statement { code: CC_POLICY_PCR, body: unhex("0000 00000001 000b 03 800000") };
    assert!(Policy::new(vec![empty_digest]).digest(HashAlg::Sha256).is_err(), "the values of the day");
}

/// PolicyAuthorize, the approved policy hash and the name, by hand from
/// part 3, 23.16 and 23.2.3, and part 1, clause 16, with Python's hashlib.
#[test]
fn the_digests_of_policy_authorize_are_part_3s() {
    let reference = [0x33u8; 32];
    let digest = policy_authorize_digest(HashAlg::Sha256, &unhex(NAME), &reference).unwrap();
    assert_eq!(digest, unhex("c35d02a67214c72b587ae44ed2f127e0beb02ce6b5844180470624b7b02a1ea7"));
    let no_ref = policy_authorize_digest(HashAlg::Sha256, &unhex(NAME), &[]).unwrap();
    assert_eq!(no_ref, unhex("842028e3d3452530e35b8886478285d59e04535af23e9249d0edfe2e337e3fdd"));
    let a_hash = approved_policy_hash(HashAlg::Sha256, &unhex(PCR7_ZERO), &reference).unwrap();
    assert_eq!(a_hash, unhex("912bdbecf642e8d4bffbf7bbd87957ddce42ea289598a1eebb0d595bb07e19bf"));
    let point = unhex(&format!("04 {} {}", "11".repeat(32), "22".repeat(32)));
    let area = public::external_p256(&point).unwrap();
    assert_eq!(policy::key_name(&area).unwrap(), unhex(NAME));
    assert!(
        matches!(policy_authorize_digest(HashAlg::Sha256, &unhex("000b 00"), &[]), Err(Error::Usage(_))),
        "a short name"
    );
    assert!(policy_authorize_digest(HashAlg::Sha256, &unhex(NAME), &[0; 65]).is_err(), "a policyRef over 64 bytes");
    assert!(approved_policy_hash(HashAlg::Sha256, &[0; 65], &[]).is_err());
    let rsa_named = unhex("0008 0001 0004 00000000");
    assert!(matches!(policy::key_name(&rsa_named), Err(Error::Unsupported(_))), "a key named with SHA-1");
}

/// A signed policy: its approved policy is the digest before its last
/// PolicyAuthorize, and the hash that the policy key signs comes from it.
/// PolicyAuthorize sets the digest again; PolicyAuthValue and PolicyPassword extend
/// with one code; a body of PolicyAuthValue is refused.
#[test]
fn a_policy_keeps_its_order_and_its_approved_digest() {
    let point = unhex(&format!("04 {} {}", "11".repeat(32), "22".repeat(32)));
    let head = Authorize {
        public: public::external_p256(&point).unwrap(),
        policy_ref: vec![0x33; 32],
        signature: public::NULL_SIGNATURE.to_vec(),
    };
    let zeros = policy::pcr_values_digest(HashAlg::Sha256, &[&[0; 32]]).unwrap();
    let pcr = Statement::pcr(&sel("7"), &zeros);
    let signed = Policy::new(vec![pcr.clone(), Statement::authorize(&head).unwrap()]);
    assert_eq!(signed.approved(HashAlg::Sha256).unwrap(), unhex(PCR7_ZERO));
    assert!(Policy::new(vec![pcr.clone()]).approved(HashAlg::Sha256).is_err());
    // PolicyAuthorize first, as a key file has it in the policy of a key:
    // the statements after it extend its digest; those before it do not count.
    let first = Policy::new(vec![Statement::authorize(&head).unwrap(), pcr.clone()]);
    let after = Policy::new(vec![Statement::authorize(&head).unwrap()]).digest(HashAlg::Sha256).unwrap();
    let want = policy::policy_pcr_update(HashAlg::Sha256, &after, &sel("7"), &zeros).unwrap();
    assert_eq!(first.digest(HashAlg::Sha256).unwrap(), want, "PolicyAuthorize first");
    let before = Policy::new(vec![pcr.clone(), Statement::authorize(&head).unwrap()]);
    assert_eq!(before.digest(HashAlg::Sha256).unwrap(), after, "the statements before PolicyAuthorize do not count");
    let value = Statement { code: CC_POLICY_AUTH_VALUE, body: vec![] };
    let password = Statement { code: CC_POLICY_PASSWORD, body: vec![] };
    let a = Policy::new(vec![pcr.clone(), value]).digest(HashAlg::Sha256).unwrap();
    let b = Policy::new(vec![pcr.clone(), password]).digest(HashAlg::Sha256).unwrap();
    assert_eq!(a, b, "PolicyPassword extends with the code of PolicyAuthValue");
    assert_eq!(a, policy::hash(HashAlg::Sha256, &[&unhex(PCR7_ZERO), &[0, 0, 1, 0x6B]]).unwrap());
    let body = Statement { code: CC_POLICY_AUTH_VALUE, body: vec![0] };
    assert!(Policy::new(vec![body]).digest(HashAlg::Sha256).is_err());
    assert_eq!(Policy::default().digest(HashAlg::Sha256).unwrap(), vec![0; 32], "no statement: zeros");
    assert_eq!(CC_POLICY_AUTHORIZE, 0x0000_016A);
}

/// Each bit of each input changes the digest: the old digest, a value of a
/// PCR, the name of the policy key, the policy reference and the approved
/// policy.
#[test]
fn each_bit_of_each_input_changes_the_digest() {
    let flips = |bytes: &[u8]| -> Vec<Vec<u8>> {
        (0..bytes.len() * 8)
            .map(|bit| {
                let mut b = bytes.to_vec();
                b[bit / 8] ^= 1 << (bit % 8);
                b
            })
            .collect()
    };
    let seven = sel("7");
    let base = policy_pcr_digest(HashAlg::Sha256, &[0; 32], &seven, &[&[0; 32]]).unwrap();
    for old in flips(&[0; 32]) {
        assert_ne!(policy_pcr_digest(HashAlg::Sha256, &old, &seven, &[&[0; 32]]).unwrap(), base);
    }
    for value in flips(&[0; 32]) {
        assert_ne!(policy_pcr_digest(HashAlg::Sha256, &[0; 32], &seven, &[&value]).unwrap(), base);
    }
    let name = unhex(NAME);
    let reference = [0x33u8; 32];
    let base = policy_authorize_digest(HashAlg::Sha256, &name, &reference).unwrap();
    for changed in flips(&name[2..]) {
        let changed = [&name[..2], &changed[..]].concat();
        assert_ne!(policy_authorize_digest(HashAlg::Sha256, &changed, &reference).unwrap(), base);
    }
    for changed in flips(&reference) {
        assert_ne!(policy_authorize_digest(HashAlg::Sha256, &name, &changed).unwrap(), base);
    }
    let approved = unhex(PCR7_ZERO);
    let base = approved_policy_hash(HashAlg::Sha256, &approved, &reference).unwrap();
    for changed in flips(&approved) {
        assert_ne!(approved_policy_hash(HashAlg::Sha256, &changed, &reference).unwrap(), base);
    }
    for changed in flips(&reference) {
        assert_ne!(approved_policy_hash(HashAlg::Sha256, &approved, &changed).unwrap(), base);
    }
}
