// **A TLV walk over a certificate, shared by every test that reads one.**
// `include!`d by `certificate_structure.rs` and, for the `which_signature_is
// _failing` diagnostic, by `hostname_verification.rs`.
//
// **A walk, not a byte-pattern search.** A DER `SEQUENCE` inside an ECDSA
// signature varies in length from signing to signing, because ECDSA's `s`
// is randomised: a search for a fixed header (`03 48 00`) finds the
// `signatureValue` BIT STRING about half the time, and the test fails as a
// missing field of the *certificate*, one run in two.
//
// Every offset here is relative to the buffer it was found in, and the
// buffer is named at each step: a range of one buffer used as an index into
// another reads as a malformed certificate.

use std::ops::Range;

// The walk is `include!`d inside `mod walk { .. }` in `certificate_structure.rs`,
// whose sibling `mod cert` re-exports the encoding helpers, so the path from
// here is `super::cert::encoding::read_len`.
use super::cert::encoding::read_len;

/// The `subjectPublicKey` exactly as webpki hands it to a verification
/// algorithm: the SPKI BIT STRING's **content**, unused-bits byte included.
/// The leading `0x04` is what `from_sec1_bytes` parses; without it each
/// handshake answers `BadSignature`. The byte before it is the BIT STRING's
/// unused-bit count.
pub fn spki_point(der: &[u8]) -> Vec<u8> {
    let (tbs_contents, _) = read_sequence(der, 0);
    let (inner_contents, _) = read_sequence(&tbs_contents, 0);
    let fields = tlv_fields(&inner_contents, "a tbsCertificate");

    // **The SPKI is the field IMMEDIATELY BEFORE the subjectPublicKeyInfo
    // position, not "the last field".** TBSCertificate is
    // version, serialNumber, signature, issuer, validity, subject, spki, and
    // then the OPTIONAL [1] issuerUniqueID, [2] subjectUniqueID and [3]
    // extensions. An anchor here carries no SAN, so it has extensions but
    // fewer of them, and "the field before the last one" is the validity or
    // the subject, not the key. Counting the REQUIRED fields gives position
    // 6, which does not move when optional fields are added.
    const SPKI_FIELD: usize = 6;
    assert!(fields.len() > SPKI_FIELD, "a tbsCertificate has a subjectPublicKeyInfo at field {SPKI_FIELD}");
    eprintln!(
        "TBS-FIELDS {} tags {:?}",
        fields.len(),
        fields.iter().map(|f| inner_contents[f.1.start]).collect::<Vec<u8>>()
    );
    let (spki_contents, _) = read_sequence(&inner_contents, fields[SPKI_FIELD].1.start);
    let inner = tlv_fields(&spki_contents, "a SubjectPublicKeyInfo");
    assert_eq!(inner.len(), 2, "an SPKI is an AlgorithmIdentifier and a key");
    inner[1].0.clone()
}

/// The 65-byte SEC1 encoding webpki ends up passing: the BIT STRING's content
/// with the unused-bits byte removed.
pub fn sec1_point(content: &[u8]) -> Vec<u8> {
    content[1..].to_vec()
}

/// A BIT STRING's bytes, minus the unused-bits count that leads it.
pub fn bitstring_bytes(content: &[u8], what: &str) -> Vec<u8> {
    assert_eq!(content[0], 0, "{what} has unused bits");
    content[1..].to_vec()
}

/// The `signatureValue` BIT STRING's bytes: the last element of the
/// certificate, reached by parsing the certificate rather than by searching it.
pub fn signature_of(der: &[u8]) -> Vec<u8> {
    let (contents, _) = read_sequence(der, 0);
    let fields = tlv_fields(&contents, "a certificate");
    assert_eq!(fields.len(), 3, "a certificate has three fields");
    bitstring_bytes(&fields[2].0, "the signatureValue")
}

/// The `tbsCertificate` TLV, exactly as `untrusted::Reader::read_partial`
/// reports it consumed — header included.
pub fn tbs_tlv_of(der: &[u8]) -> Vec<u8> {
    let (contents, _) = read_sequence(der, 0);
    let (_, after_tbs) = read_sequence(&contents, 0);
    // **The header is part of what was signed.** `read_sequence` hands back
    // the content, which is one thing a helper can reasonably return and the
    // one thing a signature must not be computed over: webpki passes the bytes
    // `read_partial` reports consumed, and that is the whole TLV. The content
    // alone (it begins `a0 03 02 01 02`, the TLV begins `30 82`) gives the
    // same `BadSignature` as a defect of the builder.
    contents[..after_tbs].to_vec()
}

/// `der[at..]`, asserted to be a SEQUENCE: returns its content and the offset
/// just past it.
pub fn read_sequence(der: &[u8], at: usize) -> (Vec<u8>, usize) {
    assert_eq!(der[at], 0x30, "expected a SEQUENCE at offset {at}");
    let (len, start) = read_len(der, at + 1);
    (der[start..start + len].to_vec(), start + len)
}

/// Split a concatenation of TLVs, returning each value's content and the range
/// its TLV occupies **within `contents`**. Panics rather than guessing: a
/// helper that returns a default offset produces a test that passes for the
/// wrong reason.
pub fn tlv_fields(contents: &[u8], what: &str) -> Vec<(Vec<u8>, Range<usize>)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < contents.len() {
        let (len, start) = read_len(contents, at + 1);
        let end = start + len;
        assert!(end <= contents.len(), "{what} overruns its container");
        out.push((contents[start..end].to_vec(), at..end));
        at = end;
    }
    out
}
