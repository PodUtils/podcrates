//! The frame of a response and its bounds, and the response codes: each
//! response is untrusted, so a short, a long or a broken one is an error,
//! never a panic.

mod common;

use common::{scripted, unhex};
use pod_core::{Class, Failure};
use pod_tpm::cmd::{self, CC_LOAD, CC_PCR_READ, CC_POLICY_GET_DIGEST};
use pod_tpm::wire::{MAX_RESPONSE, response};
use pod_tpm::{Code, Error, Reply, ResponseCode, Subject, transact};

/// A response: the code of a refusal, the tag of a TPM 1.2, a size that is
/// not the count of bytes that came, and the parameters after their own
/// size.
#[test]
fn a_response_is_read_or_refused_with_its_reason() {
    let ok = unhex("8002 00000019 00000000 80000002 00000002 0000 0000 01 0000");
    let read = response(&ok, 1).unwrap();
    assert_eq!(read.handles, vec![0x8000_0002]);
    assert_eq!(read.params.rest(), &[0, 0]);
    let refused = response(&unhex("8001 0000000a 000001c4"), 0).err().unwrap();
    assert_eq!(refused.code(), Some(ResponseCode::new(0x1C4)));
    let old = response(&unhex("00c4 0000000a 00000000"), 0).err().unwrap().to_string();
    assert!(old.contains("TPM 1.2"), "{old}");
    let short = response(&unhex("8001 0000000c 00000000"), 0).err().unwrap().to_string();
    assert!(short.contains("12 bytes, and 10 came"), "{short}");
    assert!(response(&unhex("8001 0000000a 00000000"), 1).is_err(), "a handle due and none came");
    assert!(response(&[0x80], 0).is_err());
    let tag = response(&unhex("8003 0000000a 00000000"), 0).err().unwrap().to_string();
    assert!(tag.contains("0x8003"), "{tag}");
}

/// A response of more than `MAX_RESPONSE` bytes is refused, also when its
/// size field says so; one of exactly `MAX_RESPONSE` bytes is read.
#[test]
fn a_response_over_the_bound_of_the_frame_is_refused() {
    let frame = |len: usize| {
        let mut bytes = unhex("8001");
        bytes.extend_from_slice(&(len as u32).to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.resize(len, 0);
        bytes
    };
    let full = frame(MAX_RESPONSE);
    let at = response(&full, 0).unwrap();
    assert_eq!(at.params.remaining(), MAX_RESPONSE - 10);
    let over = response(&frame(MAX_RESPONSE + 1), 0).err().unwrap();
    assert!(matches!(over, Error::Malformed(_)), "{over}");
    assert!(over.to_string().contains("more than the 4096"), "{over}");
    assert_eq!(over.class(), Class::Protocol);
}

/// Each cut of a good response, and a byte more, is refused with an error.
#[test]
fn a_short_or_a_long_response_is_an_error() {
    let digest = format!("8001 0000002c 00000000 0020 {}", "ab".repeat(32));
    let goods = [
        (CC_LOAD, unhex("8002 00000019 00000000 80000002 00000002 0000 0000 01 0000")),
        (CC_POLICY_GET_DIGEST, unhex(&digest)),
    ];
    for (code, good) in goods {
        let reply = Reply::read(code, &good).unwrap();
        assert!(reply.params.len() <= good.len());
        for cut in 0..good.len() {
            assert!(Reply::read(code, &good[..cut]).is_err(), "{} cut at {cut}", cmd::name(code));
        }
        let mut longer = good.clone();
        longer.push(0);
        assert!(Reply::read(code, &longer).is_err(), "{} with a byte more", cmd::name(code));
    }
    // The size field agrees, and the parameters hold a byte after the digest.
    let mut extra = unhex(&digest);
    extra.push(0x99);
    extra[5] += 1;
    let reply = Reply::read(CC_POLICY_GET_DIGEST, &extra).unwrap();
    assert!(cmd::policy_digest(&reply).is_err(), "a byte after the digest");
}

/// A sized buffer over the bound of part 2, and a count that passes the
/// bytes that remain, are refused before a read or an allocation.
#[test]
fn a_sized_buffer_or_a_count_past_its_bound_is_refused() {
    let digest = format!("8001 0000004d 00000000 0041 {}", "ab".repeat(65));
    let reply = Reply::read(CC_POLICY_GET_DIGEST, &unhex(&digest)).unwrap();
    let e = cmd::policy_digest(&reply).unwrap_err().to_string();
    assert!(e.contains("65 bytes, where part 2 allows 64"), "{e}");
    let many = unhex("8001 00000016 00000000 00000007 00000000 ffffffff");
    let reply = Reply::read(CC_PCR_READ, &many).unwrap();
    let e = cmd::PcrValues::read(&reply).unwrap_err().to_string();
    assert!(e.contains("4294967295"), "{e}");
    let banks = unhex("8001 00000014 00000000 00000007 00000011 0000");
    let e = cmd::PcrValues::read(&Reply::read(CC_PCR_READ, &banks).unwrap()).unwrap_err().to_string();
    assert!(e.contains("17 PCR banks, where part 2 allows 16"), "{e}");
    let past = unhex("8001 00000015 00000000 00000007 00000003 000b03");
    let e = cmd::PcrValues::read(&Reply::read(CC_PCR_READ, &past).unwrap()).unwrap_err().to_string();
    assert!(e.contains("cannot hold them"), "{e}");
    let params = unhex("8002 00000012 00000000 80000002 ffffffff");
    assert!(Reply::read(CC_LOAD, &params).is_err(), "a parameter size past the bytes");
}

/// Each byte of each good response, changed, gives a reply or an error,
/// never a panic.
#[test]
fn each_changed_byte_of_a_response_is_read_or_refused() {
    let goods = [
        (cmd::CC_CREATE_PRIMARY, unhex(PRIMARY)),
        (
            cmd::CC_PCR_READ,
            unhex(&format!(
                "8001 0000003e 00000000 00000007 00000001 000b 03 800000 00000001 0020 {}",
                "55".repeat(32)
            )),
        ),
        (cmd::CC_GET_CAPABILITY, unhex("8001 0000001b 00000000 00 00000006 00000001 00000105 49465800")),
    ];
    for (code, good) in goods {
        for byte in 0..good.len() {
            for flip in [0x01, 0x80, 0xFF] {
                let mut changed = good.clone();
                changed[byte] ^= flip;
                if let Ok(reply) = Reply::read(code, &changed) {
                    let _ = cmd::Primary::read(&reply);
                    let _ = cmd::PcrValues::read(&reply);
                    let _ = cmd::Capability::read(&reply);
                }
            }
        }
    }
}

/// A response of `TPM2_CreatePrimary`, by hand from part 3, 24.1: the
/// handle, the size of the parameters, the public area, an empty creation
/// data and hash, a creation ticket, the name, then the authorization of
/// the TPM.
pub const PRIMARY: &str =
    "8002 0000002d 00000000 80000001 00000016 0004 0023 000b 0000 0000 8021 40000001 0000 0002 000b 0000 01 0000";

/// Each code in words, with what it is of.
#[test]
fn each_code_says_its_name_and_its_place() {
    let cases = [
        (0x1C4, "TPM_RC_VALUE", "parameter 1"),
        (0x99D, "TPM_RC_POLICY_FAIL", "session 1"),
        (0x9A2, "TPM_RC_BAD_AUTH", "session 1"),
        (0x18B, "TPM_RC_HANDLE", "handle 1"),
        (0x1DF, "TPM_RC_INTEGRITY", "parameter 1"),
        (0x12F, "TPM_RC_AUTH_UNAVAILABLE", "0x012f"),
        (0x921, "TPM_RC_LOCKOUT", "0x0921"),
        (0x0001, "TPM 1.2", "0x0001"),
        (0x000B_0005, "resource manager", "layer 11"),
        (0x0BB, "an error of the TPM", "0x00bb"),
        (0x500, "vendor", "0x0500"),
    ];
    for (code, name, place) in cases {
        let line = ResponseCode::new(code).to_string();
        assert!(line.contains(name) && line.contains(place), "{code:#x}: {line}");
        assert!(!line.contains('\n'), "one line: {line}");
    }
    assert_eq!(ResponseCode::new(0x1C4).code(), Code::Value);
    assert_eq!(ResponseCode::new(0x99D).code(), Code::PolicyFail);
    assert_eq!(ResponseCode::new(0x9A2).code(), Code::BadAuth);
    assert_eq!(ResponseCode::new(0x921).code(), Code::Lockout);
    assert_eq!(ResponseCode::new(0x0BB).code(), Code::Unknown(0x0BB));
    assert_eq!(ResponseCode::new(0x1C4).subject(), Some(Subject::Parameter(1)));
    assert_eq!(ResponseCode::new(0x99D).subject(), Some(Subject::Session(1)));
    assert_eq!(ResponseCode::new(0x28B).subject(), Some(Subject::Handle(2)));
    assert_eq!(ResponseCode::new(0x08B).subject(), None, "no number");
    assert_eq!(ResponseCode::new(0x128).subject(), None, "format zero");
}

/// Each code of the table reads back as itself, alone and with each
/// modifier of the format one; each has a name of part 2 and words.
#[test]
fn each_code_of_the_table_reads_back() {
    let mut count = 0;
    for code in Code::all() {
        count += 1;
        let value = code.value();
        assert_eq!(ResponseCode::new(value).code(), code, "{value:#x}");
        assert!(code.name().is_some_and(|n| n.starts_with("TPM_RC_")), "{value:#x}");
        assert!(!code.words().is_empty());
        if value & 0x080 != 0 {
            for modifier in [0x100, 0x140, 0x900, 0x740, 0xF40] {
                assert_eq!(ResponseCode::new(value | modifier).code(), code, "{value:#x} with {modifier:#x}");
            }
        }
    }
    assert!(count > 90, "{count} codes");
}

/// The class of a code: a refusal of the policy, a warning, an algorithm
/// that the TPM does not have, a layer in front of the TPM, a TPM 1.2.
#[test]
fn each_code_has_the_class_of_its_meaning() {
    let cases = [
        (0x99D, Class::Policy),
        (0x128, Class::Policy),
        (0x921, Class::Policy),
        (0x9A2, Class::Policy),
        (0x1DF, Class::Policy),
        (0x922, Class::TempFail),
        (0x902, Class::TempFail),
        (0x143, Class::Unsupported),
        (0x2E6, Class::Unsupported),
        (0x101, Class::Unavailable),
        (0x910, Class::Usage),
        (0x1C4, Class::Protocol),
        (0x000B_0005, Class::Unavailable),
        (0x0001, Class::Unsupported),
    ];
    for (code, class) in cases {
        assert_eq!(ResponseCode::new(code).class(), class, "{code:#x}");
        let failure: Failure = Error::Tpm(ResponseCode::new(code)).into();
        assert_eq!(failure.class, class, "{code:#x}");
        assert_eq!(failure.stage, pod_core::Stage::Host);
    }
}

/// A closure is a transport: `transact` sends the command, reads the
/// handles that its code gives, and keeps the class of a failed transport.
#[test]
fn a_closure_is_a_transport() {
    let mut seen = Vec::new();
    let answers =
        vec![unhex("8002 00000019 00000000 80000002 00000002 0000 0000 01 0000"), unhex("8001 0000000a 0000098e")];
    let mut tpm = scripted(answers, &mut seen);
    let load = cmd::load(0x8100_0001, &unhex("0000"), &unhex("0000")).unwrap();
    let reply = transact(&mut tpm, &load).unwrap();
    assert_eq!(cmd::Loaded::read(&reply).unwrap(), cmd::Loaded { handle: 0x8000_0002, name: vec![] });
    let refused = transact(&mut tpm, &load).unwrap_err();
    assert_eq!(refused.code().map(|c| c.code()), Some(Code::AuthFail));
    let gone = transact(&mut tpm, &load).unwrap_err();
    assert_eq!(gone.class(), Class::Unavailable);
    assert!(matches!(transact(&mut tpm, &[0x80, 0x01]), Err(Error::Usage(_))), "a command shorter than its header");
    drop(tpm);
    assert_eq!(seen.len(), 3, "the short command is not sent");
}
