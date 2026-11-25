//! The key file is untrusted input: each refusal by its name (a sealed or
//! an importable key, a volatile parent, an oversize field, bytes after a
//! structure, a rule of DER or of the draft that is broken), and each cut
//! and each bit flip of a good file is refused or read back to the same
//! bytes, never a panic.

mod common;

use common::{LOADABLE, der_int, der_key, der_statement, tlv, unhex};
use pod_core::Class;
use pod_tpm::Error;
use pod_tpm::cmd::{CC_POLICY_AUTHORIZE, CC_POLICY_PCR};
use pod_tpm::keyfile::{self, AuthPolicy, KeyFile, MAX_DER, MAX_PEM, Refusal};
use pod_tpm::wire::RH_OWNER;
use pod_tpm::{Policy, Statement};

/// The refusal of a key file, or a panic with what came.
fn refusal(result: Result<KeyFile, Error>) -> Refusal {
    match result {
        Err(Error::KeyFile(refusal)) => refusal,
        other => panic!("not a refusal of a key file: {other:?}"),
    }
}

fn read(der: &[u8]) -> Refusal {
    refusal(KeyFile::from_der(der))
}

fn write(file: &KeyFile) -> Refusal {
    match file.to_der() {
        Err(Error::KeyFile(refusal)) => refusal,
        other => panic!("the writer took a file that the reader refuses: {other:?}"),
    }
}

fn statement(code: u32, body: &[u8]) -> Statement {
    Statement { code, body: body.to_vec() }
}

/// A good key under one signed policy, as the reader takes it.
fn good() -> Vec<u8> {
    let signed = tlv(
        0x30,
        &[tlv(0xA0, &tlv(0x0C, b"n")), tlv(0xA1, &tlv(0x30, &der_statement(CC_POLICY_AUTHORIZE, &[2])))].concat(),
    );
    let fields = [
        tlv(0xA1, &tlv(0x30, &der_statement(CC_POLICY_AUTHORIZE, &[1]))),
        tlv(0xA3, &tlv(0x30, &signed)),
        tlv(0xA4, &tlv(0x0C, b"a key")),
    ];
    der_key(&fields, 0x8100_0001)
}

/// A sealed and an importable key are read and refused by their names; an
/// unknown type is refused with its OID.
#[test]
fn a_sealed_or_an_importable_key_is_refused_by_its_name() {
    let mut sealed = der_key(&[], RH_OWNER);
    sealed[9] = 0x05;
    let why = read(&sealed);
    assert_eq!(why, Refusal::Sealed);
    let error = KeyFile::from_der(&sealed).unwrap_err();
    assert_eq!(error.class(), Class::Unsupported);
    assert!(error.to_string().contains("TPM2_Unseal"), "{error}");
    let mut importable = der_key(&[], RH_OWNER);
    importable[9] = 0x04;
    assert_eq!(read(&importable), Refusal::Importable);
    let mut other = der_key(&[], RH_OWNER);
    other[9] = 0x09;
    assert_eq!(read(&other), Refusal::UnknownType("2.23.133.10.1.9".into()));
    assert!(KeyFile::from_der(&other).unwrap_err().to_string().contains("2.23.133.10.1.9"));
}

/// The parent: a volatile handle (MSO 0x80) and another MSO are refused,
/// by the reader and by the writer; rsaParent only with a permanent one.
#[test]
fn a_parent_that_is_a_volatile_handle_is_refused() {
    assert_eq!(read(&der_key(&[], 0x8000_0001)), Refusal::VolatileParent(0x8000_0001));
    let error = KeyFile::from_der(&der_key(&[], 0x8000_0001)).unwrap_err();
    assert_eq!(error.class(), Class::Config);
    assert!(error.to_string().contains("volatile"), "{error}");
    assert_eq!(write(&KeyFile::loadable(0x8000_0001, vec![0, 0], vec![0, 0])), Refusal::VolatileParent(0x8000_0001));
    assert_eq!(read(&der_key(&[], 0x0100_0000)), Refusal::BadParent(0x0100_0000));
    assert_eq!(read(&der_key(&[], 0x0300_0000)), Refusal::BadParent(0x0300_0000));
    for parent in [RH_OWNER, 0x4000_000B, 0x8100_0001, 0x81FF_FFFF] {
        assert_eq!(KeyFile::from_der(&der_key(&[], parent)).unwrap().parent, parent);
    }
    let rsa = tlv(0xA5, &tlv(0x01, &[0xFF]));
    assert!(KeyFile::from_der(&der_key(std::slice::from_ref(&rsa), RH_OWNER)).unwrap().rsa_parent);
    assert!(matches!(read(&der_key(&[rsa], 0x8100_0001)), Refusal::Malformed(why) if why.contains("rsaParent")));
}

/// Each field over its bound, by the reader and by the writer.
#[test]
fn an_oversize_field_is_refused() {
    let mut big = vec![0x0F, 0xFF];
    big.resize(4097, 0);
    let file = KeyFile::loadable(RH_OWNER, big.clone(), vec![0, 0]);
    assert!(matches!(write(&file), Refusal::TooLarge { field: "pubkey", size: 4097, max: 4096 }));
    let file = KeyFile::loadable(RH_OWNER, vec![0, 0], big.clone());
    assert!(matches!(write(&file), Refusal::TooLarge { field: "privkey", .. }));
    let body = [LOADABLE, &der_int(RH_OWNER), &tlv(0x04, &big), &tlv(0x04, &[0, 0])].concat();
    assert!(matches!(read(&tlv(0x30, &body)), Refusal::TooLarge { field: "pubkey", .. }));

    let mut file = KeyFile::loadable(RH_OWNER, vec![0, 0], vec![0, 0]);
    file.description = Some("d".repeat(257));
    assert!(matches!(write(&file), Refusal::TooLarge { field: "description", .. }));
    let long = tlv(0xA4, &tlv(0x0C, &[b'd'; 257]));
    assert!(matches!(read(&der_key(&[long], RH_OWNER)), Refusal::TooLarge { field: "description", .. }));

    let mut file = KeyFile::loadable(RH_OWNER, vec![0, 0], vec![0, 0]);
    file.policy = Policy::new(vec![statement(CC_POLICY_PCR, &[0; 1025])]);
    assert!(matches!(write(&file), Refusal::TooLarge { field: "the body of a policy statement", .. }));
    let long = der_key(&[tlv(0xA1, &tlv(0x30, &der_statement(CC_POLICY_PCR, &[0; 1025])))], RH_OWNER);
    assert!(matches!(read(&long), Refusal::TooLarge { field: "the body of a policy statement", .. }));
    file.policy = Policy::new(vec![statement(CC_POLICY_PCR, &[0]); 9]);
    assert!(matches!(write(&file), Refusal::TooMany { max: 8, .. }));
    let nine: Vec<u8> = (0..9).flat_map(|_| der_statement(CC_POLICY_PCR, &[0])).collect();
    assert!(matches!(read(&der_key(&[tlv(0xA1, &tlv(0x30, &nine))], RH_OWNER)), Refusal::TooMany { max: 8, .. }));

    file.policy = Policy::new(vec![statement(CC_POLICY_AUTHORIZE, &[0])]);
    let signed = AuthPolicy { name: None, policy: Policy::new(vec![statement(CC_POLICY_AUTHORIZE, &[0])]) };
    file.auth_policy = vec![signed; 9];
    assert!(matches!(write(&file), Refusal::TooMany { field: "signed policies", max: 8 }));
    let one = tlv(0x30, &tlv(0xA1, &tlv(0x30, &der_statement(CC_POLICY_AUTHORIZE, &[0]))));
    let fields =
        [tlv(0xA1, &tlv(0x30, &der_statement(CC_POLICY_AUTHORIZE, &[0]))), tlv(0xA3, &tlv(0x30, &one.repeat(9)))];
    assert!(matches!(read(&der_key(&fields, RH_OWNER)), Refusal::TooMany { field: "signed policies", max: 8 }));

    let huge = der_key(&[tlv(0xA4, &tlv(0x0C, &vec![b'd'; MAX_DER]))], RH_OWNER);
    assert!(matches!(read(&huge), Refusal::TooLarge { field: "the key file", .. }));
    let text = format!("-----BEGIN TSS2 PRIVATE KEY-----\n{}\n-----END TSS2 PRIVATE KEY-----\n", "A".repeat(MAX_PEM));
    assert!(matches!(refusal(KeyFile::from_pem(&text)), Refusal::TooLarge { field: "the PEM text", .. }));
}

/// A byte after the TPMKey, after privkey, in a statement, in an explicit
/// field, and in the TPM2B of pubkey.
#[test]
fn trailing_bytes_are_refused() {
    let mut longer = good();
    longer.push(0);
    assert!(matches!(read(&longer), Refusal::Trailing { count: 1, .. }));
    let message = KeyFile::from_der(&longer).unwrap_err().to_string();
    assert!(message.contains("1 byte"), "{message}");
    let body = [LOADABLE, &der_int(RH_OWNER), &tlv(0x04, &[0, 0]), &tlv(0x04, &[0, 0]), &[0x05, 0x00][..]].concat();
    assert!(matches!(read(&tlv(0x30, &body)), Refusal::Trailing { count: 2, .. }));
    let step = tlv(0x30, &[tlv(0xA0, &der_int(CC_POLICY_PCR)), tlv(0xA1, &tlv(0x04, &[1])), vec![0x05, 0]].concat());
    assert!(matches!(read(&der_key(&[tlv(0xA1, &tlv(0x30, &step))], RH_OWNER)), Refusal::Trailing { .. }));
    let empty_auth = tlv(0xA0, &[tlv(0x01, &[0xFF]), vec![0x05, 0]].concat());
    assert!(matches!(read(&der_key(&[empty_auth], RH_OWNER)), Refusal::Trailing { .. }));
    let body = [LOADABLE, &der_int(RH_OWNER), &tlv(0x04, &[0, 0, 7]), &tlv(0x04, &[0, 0])].concat();
    assert!(matches!(read(&tlv(0x30, &body)), Refusal::Malformed(why) if why.contains("pubkey")));
    let file = KeyFile::loadable(RH_OWNER, vec![0, 1], vec![0, 0]);
    assert!(matches!(write(&file), Refusal::Malformed(why) if why.contains("pubkey")));
}

/// The rules of DER and of the draft: definite lengths in their fewest
/// octets, one-octet tags, integers in their fewest octets, each field once
/// and in order, no field that the draft does not define, no secret in a
/// loadable key, and the layout of the signed policies.
#[test]
fn the_rules_of_der_and_of_the_draft_are_kept() {
    let malformed = |der: &[u8], words: &str| match read(der) {
        Refusal::Malformed(why) => assert!(why.contains(words), "{why} lacks {words:?}"),
        other => panic!("{other:?}, where a refusal with {words:?} was due"),
    };
    malformed(&unhex("3080 0000"), "indefinite");
    malformed(&unhex("3081 05 0101ff0500"), "fewest");
    malformed(&unhex("1f81 0100"), "tag");
    malformed(&unhex("3084 00000001 00"), "length");
    malformed(&unhex("3005 0101ff"), "past the end");
    let auth = tlv(0xA0, &tlv(0x01, &[0xFF]));
    malformed(&der_key(&[auth.clone(), auth.clone()], RH_OWNER), "order");
    let text = tlv(0xA4, &tlv(0x0C, b"d"));
    malformed(&der_key(&[text, auth.clone()], RH_OWNER), "order");
    assert_eq!(read(&der_key(&[tlv(0xA6, &tlv(0x05, &[]))], RH_OWNER)), Refusal::UnknownField(6));
    malformed(&der_key(&[tlv(0xA2, &tlv(0x04, &[0, 0]))], RH_OWNER), "secret");
    let body = [LOADABLE, &tlv(0x02, &[0x00, 0x40, 0, 0, 1]), &tlv(0x04, &[0, 0]), &tlv(0x04, &[0, 0])].concat();
    malformed(&tlv(0x30, &body), "fewest");
    let body = [LOADABLE, &tlv(0x02, &[0xC0, 0, 0, 1]), &tlv(0x04, &[0, 0]), &tlv(0x04, &[0, 0])].concat();
    malformed(&tlv(0x30, &body), "negative");
    malformed(&der_key(&[tlv(0xA0, &tlv(0x01, &[0xFF, 0xFF]))], RH_OWNER), "BOOLEAN");
    malformed(&der_key(&[tlv(0xA4, &tlv(0x0C, &[0xFF, 0xFE]))], RH_OWNER), "UTF-8");
    // A BOOLEAN of a non-zero octet other than 0xFF is true (the draft, 6).
    assert!(KeyFile::from_der(&der_key(&[tlv(0xA0, &tlv(0x01, &[1]))], RH_OWNER)).unwrap().empty_auth);

    let one = |code| tlv(0x30, &der_statement(code, &[1]));
    let signed = |steps: Vec<u8>| tlv(0xA3, &tlv(0x30, &tlv(0x30, &tlv(0xA1, &tlv(0x30, &steps)))));
    let head = tlv(0xA1, &one(CC_POLICY_AUTHORIZE));
    malformed(&der_key(&[signed(der_statement(CC_POLICY_AUTHORIZE, &[2]))], RH_OWNER), "authPolicy");
    malformed(
        &der_key(&[tlv(0xA1, &one(CC_POLICY_PCR)), signed(der_statement(CC_POLICY_AUTHORIZE, &[2]))], RH_OWNER),
        "first",
    );
    malformed(&der_key(&[head.clone(), signed(der_statement(CC_POLICY_PCR, &[2]))], RH_OWNER), "end");
    let middle = [
        der_statement(CC_POLICY_PCR, &[1]),
        der_statement(CC_POLICY_AUTHORIZE, &[2]),
        der_statement(CC_POLICY_AUTHORIZE, &[3]),
    ];
    malformed(&der_key(&[head.clone(), signed(middle.concat())], RH_OWNER), "PolicyAuthorize");
    let two = [
        der_statement(CC_POLICY_AUTHORIZE, &[1]),
        der_statement(CC_POLICY_PCR, &[2]),
        der_statement(CC_POLICY_AUTHORIZE, &[3]),
    ];
    assert!(
        KeyFile::from_der(&der_key(&[head.clone(), signed(two.concat())], RH_OWNER)).is_ok(),
        "an initial PolicyAuthorize"
    );
    let late = [der_statement(CC_POLICY_PCR, &[1]), der_statement(CC_POLICY_AUTHORIZE, &[2])];
    malformed(&der_key(&[tlv(0xA1, &tlv(0x30, &late.concat()))], RH_OWNER), "PolicyAuthorize");
}

/// A PEM text with no guard lines, or with a body that is not Base64, is
/// refused; another PEM block is no key file of a TPM.
#[test]
fn a_bad_pem_text_is_refused() {
    let bad = |text: &str| assert!(matches!(refusal(KeyFile::from_pem(text)), Refusal::Malformed(_)), "{text}");
    bad("-----BEGIN TSS2 PRIVATE KEY-----\n!!\n-----END TSS2 PRIVATE KEY-----\n");
    bad("-----BEGIN TSS2 PRIVATE KEY-----\nMAA=\n");
    bad("-----BEGIN OPENSSH PRIVATE KEY-----\nMAA=\n-----END OPENSSH PRIVATE KEY-----\n");
    bad("-----BEGIN TSS2 PRIVATE KEY-----\nMA\n-----END TSS2 PRIVATE KEY-----\n");
    assert!(!keyfile::is_pem(b"-----BEGIN OPENSSH PRIVATE KEY-----"));
    assert!(keyfile::is_pem(b"\n \t-----BEGIN TSS2 PRIVATE KEY-----"));
    assert!(matches!(refusal(KeyFile::read(&[0xFF, 0xFE])), Refusal::Malformed(_)));
}

/// Each cut of a good file is refused; each bit flip is refused, or read
/// to a file whose DER is the changed bytes. No input panics.
#[test]
fn each_cut_and_each_bit_flip_is_refused_or_read_back() {
    let der = good();
    let file = KeyFile::from_der(&der).unwrap();
    assert_eq!(file.to_der().unwrap(), der);
    for cut in 0..der.len() {
        assert!(KeyFile::from_der(&der[..cut]).is_err(), "cut at {cut}");
    }
    let mut taken = 0;
    for bit in 0..der.len() * 8 {
        let mut changed = der.clone();
        changed[bit / 8] ^= 1 << (bit % 8);
        if let Ok(file) = KeyFile::from_der(&changed) {
            assert_eq!(file.to_der().unwrap(), changed, "bit {bit}");
            taken += 1;
        }
    }
    assert!(taken > 0 && taken < der.len() * 8, "{taken}");
}
