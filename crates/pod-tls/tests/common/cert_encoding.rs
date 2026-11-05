// **The DER encoding and the certificate minted from it.** Included by
// `cert_for_test.rs`; not a test target of its own.
//
// **Each structural rule that webpki holds a certificate to is written
// beside the line that keeps it.** A broken rule shows as
// `InvalidCertificate(BadEncoding)` or as `BadSignature`, and neither error
// names the field that was wrong.

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use sha2::{Digest, Sha256};

/// **Short form below 128 bytes, long form above.** A length in the wrong
/// form is a parse error, and this is the one place a hand-built DER encoder
/// can be wrong for every certificate rather than for one field.
pub(crate) fn der_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len <= 0xff {
        vec![0x81, len as u8]
    } else if len <= 0xffff {
        vec![0x82, (len >> 8) as u8, len as u8]
    } else {
        // **The 3-byte form exists.** With the three branches above alone, a
        // certificate past 255 bytes gets a wrong length, and each parse of it
        // fails with `BadEncoding` from a *length field*, away from the
        // defect.
        vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
    }
}

/// **One length reader for the whole file.** Five copies of this loop is
/// five chances to disagree about where the contents start, and that
/// disagreement is invisible: every one of them parses a certificate and only
/// one of them signs the bytes the other four extracted. Returns
/// `(content length, offset at which the content starts)`.
#[allow(dead_code)]
pub(crate) fn read_len(der: &[u8], at: usize) -> (usize, usize) {
    let first = der[at];
    if first & 0x80 == 0 {
        (first as usize, at + 1)
    } else {
        let n = (first & 0x7f) as usize;
        let v = der[at + 1..at + 1 + n].iter().fold(0usize, |a, b| (a << 8) | *b as usize);
        (v, at + 1 + n)
    }
}

pub(crate) fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&der_len(contents.len()));
    out.extend_from_slice(contents);
    out
}

pub(crate) fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

pub(crate) fn set_of(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x31, &parts.concat())
}

/// **The first two arcs are packed into one byte; every later arc is a
/// base-128 varint.** This is the encoding X.509 uses and the one thing a
/// hand-rolled OID writer gets wrong.
pub(crate) fn oid(arcs: &[u64]) -> Vec<u8> {
    let mut body = vec![(arcs[0] * 40 + arcs[1]) as u8];
    for arc in &arcs[2..] {
        let mut stack = Vec::new();
        let mut v = *arc;
        loop {
            stack.push((v & 0x7f) as u8);
            v >>= 7;
            if v == 0 {
                break;
            }
        }
        stack.reverse();
        for (i, b) in stack.iter().enumerate() {
            body.push(if i + 1 < stack.len() { b | 0x80 } else { *b });
        }
    }
    tlv(0x06, &body)
}

pub(crate) struct TestCert {
    pub der: Vec<u8>,
    pub key_der: Vec<u8>,
}

/// Mint a self-signed end-entity certificate whose SAN carries `dns_name`.
pub fn self_signed(dns_name: &str) -> TestCert {
    self_signed_inner(dns_name, false)
}

/// The name `server_for` gives the anchor, in one place: the leaf's issuer
/// and the anchor's subject must be built from the same string.
///
/// A certificate with `basicConstraints CA:TRUE` IS a CA, and a server may
/// not present a CA as its end-entity certificate (webpki:
/// `OtherError(CaUsedAsEndEntity)`). So the anchor and the leaf are two
/// certificates, with the anchor one level up.
pub(crate) fn anchor_name(dns_name: &str) -> String {
    format!("{dns_name}-anchor")
}

/// The seed for a role's key. **The anchor and the leaf get DIFFERENT
/// seeds**, so they are different keys and the leaf's signature needs the
/// anchor's public key to verify.
fn role_seed(dns_name: &str, is_ca: bool) -> Vec<u8> {
    let mut s = dns_name.as_bytes().to_vec();
    s.push(if is_ca { 0xca } else { 0xee });
    s
}

pub(crate) fn self_signed_inner(dns_name: &str, is_ca: bool) -> TestCert {
    // **The SPKI publishes this certificate's OWN key; the signature below is
    // made by the ISSUER's key, and for an anchor those are the same one.**
    // A client verifies the peer's `CertificateVerify` with the key of the
    // leaf's own SPKI, but the chain with the *issuer's*: webpki's
    // `check_signed_chain` (`verify_cert.rs`) verifies each `SignedData`
    // against the issuer's SPKI. A leaf signed with its own key, under an
    // `issuer` that names the anchor, ends each handshake in `BadSignature`.
    let own = SigningKey::from_bytes(&sha256(&role_seed(dns_name, is_ca)).into()).expect("a P-256 scalar");

    let spki = own.verifying_key().to_sec1_point(false);
    let point = spki.as_bytes();
    eprintln!(
        "PUBLISHED-POINT {} head {}",
        point.len(),
        point[..8].iter().map(|b| format!("{b:02x}")).collect::<String>()
    );

    let spki_der = seq(&[
        seq(&[oid(&[1, 2, 840, 10045, 2, 1]), oid(&[1, 2, 840, 10045, 3, 1, 7])]),
        tlv(0x03, &[&[0x00][..], point].concat()),
    ]);

    let name = seq(&[set_of(&[seq(&[oid(&[2, 5, 4, 3]), tlv(0x0c, dns_name.as_bytes())])])]);
    // **The leaf's ISSUER is the anchor's SUBJECT.** A chain is walked by
    // matching the leaf's issuer against the next certificate's subject; with
    // the leaf's own name in both, webpki answers `UnknownIssuer`. The anchor
    // is minted with `{dns_name}-anchor` as its subject, so the leaf's issuer
    // names that.
    let issuer_name = if is_ca {
        name.clone()
    } else {
        seq(&[set_of(&[seq(&[oid(&[2, 5, 4, 3]), tlv(0x0c, anchor_name(dns_name).as_bytes())])])])
    };
    // **`GeneralNames` is `SEQUENCE OF`, tag `0x30`, never SET OF (`0x31`).**
    // webpki reads the extension's `extnValue` (an OCTET STRING) as the
    // `GeneralNames` sequence's contents and then calls `GeneralName::from_der`
    // on the next element directly: a `0x31` where it requires a `0x82`
    // (`dNSName`, context-specific primitive 2) is
    // `InvalidCertificate(BadEncoding)`. RFC 5280 §4.2.1.6 spells
    // `GeneralNames ::= SEQUENCE SIZE (1..MAX) OF GeneralName`.
    let san = seq(&[oid(&[2, 5, 29, 17]), tlv(0x04, &seq(&[tlv(0x82, dns_name.as_bytes())]))]);
    // **`basicConstraints: CA:TRUE`, critical, on the anchor.** The anchor is
    // its own trust anchor, and a v3 anchor with no `basicConstraints` is a
    // leaf by RFC 5280's rules (webpki: `BadEncoding`).
    // **`CA:TRUE` only on the anchor.** On the end-entity certificate webpki
    // refuses the handshake with `OtherError(CaUsedAsEndEntity)`: the parse is
    // correct and the certificate's **role** is wrong. A leaf would carry an
    // empty `SEQUENCE` here — `30 00` — which is how RFC 5280 spells "not a
    // CA"; the leaf below carries no `basicConstraints` at all.
    // **`keyUsage` is not emitted at all.** `digitalSignature | keyCertSign`
    // written by hand is easy to get wrong (a BIT STRING whose unused-bits
    // byte and named bits disagree with the length that webpki computes is
    // `BadEncoding`), and nothing here needs it.
    let basic_constraints = seq(&[
        oid(&[2, 5, 29, 19]),
        tlv(0x01, &[0xff]),
        tlv(0x04, &seq(&[if is_ca { tlv(0x01, &[0xff]) } else { Vec::new() }])),
    ]);

    // **The `signatureAlgorithm` is `ecdsa-with-SHA256`, and that is not
    // cosmetic.** A certificate whose signature algorithm contradicts its key
    // (`sha256WithRSAEncryption` over an EC public key) is refused whole with
    // `BadEncoding`.
    let sig_alg = oid(&[1, 2, 840, 10045, 4, 3, 2]);

    let mut tbs_body = vec![0xa0, 0x03, 0x02, 0x01, 0x02]; // [0] { INTEGER 2 } = v3
    tbs_body.extend_from_slice(&[0x02, 0x01, 0x01]); // serialNumber 1
    tbs_body.extend_from_slice(&seq(std::slice::from_ref(&sig_alg))); // signature
    tbs_body.extend_from_slice(&issuer_name); // issuer
    tbs_body.extend_from_slice(&seq(&[
        // **Both dates are UTCTime, and the year width follows the tag.** A
        // year from 2050 on is `GeneralizedTime` (tag 0x18, fourteen
        // characters); with a two-digit year it is `BadEncoding`. 2020-01-01
        // to 2040-01-01 is UTCTime on both ends and is unambiguous.
        tlv(0x17, b"200101000000Z"),
        tlv(0x17, b"400101000000Z"),
    ]));
    tbs_body.extend_from_slice(&name); // subject
    tbs_body.extend_from_slice(&spki_der);
    // **`[3]` holds the extensions, at the end of `tbsCertificate`.**
    // **The end-entity certificate has NO `basicConstraints` at all.**
    // webpki's `check_basic_constraints` (`verify_cert.rs`) calls
    // `bool::from_der` unconditionally when the extension is present, so the
    // empty `SEQUENCE` that spells "not a CA" is a parse error, and a leaf
    // carrying one answers `BadEncoding`. An absent extension means "not a
    // CA" as well, and it is the form every real end-entity certificate uses.
    let mut extensions = Vec::new();
    if is_ca {
        extensions.push(basic_constraints);
    } else {
        extensions.push(san);
    }
    tbs_body.extend_from_slice(&tlv(0xa3, &seq(&extensions)));

    let tbs = seq(&[tbs_body]);

    // **Sign the full tbsCertificate TLV, tag and length included, and sign
    // it as a MESSAGE — never wrap it in a DigestInfo first.**
    //
    // RFC 5280 §4.1.1.2 says the signature is over the DER encoding of
    // `TBSCertificate`, which includes its own header.
    //
    // `rustls-webpki` reaches `SignatureVerificationAlgorithm::verify_signature`
    // with the bytes that `untrusted::Reader::read_partial` reports as
    // **consumed**: the whole TLV, not the stripped value. And `DigestInfo`
    // is the *signer's* internal prehash format, not a message:
    // `Signer::sign` hashes whatever it is given, so a signature over
    // `DigestInfo(SHA256(tbs))` never agrees with a verifier that hashes `tbs`
    // once. Each fault is a `BadSignature` that names nothing.
    // **Signed by the ISSUER's key, which for an end-entity certificate is
    // not its own.** webpki verifies the leaf with the *anchor's* public key.
    //
    // **The signature being self-consistent is not the same as the chain
    // being consistent.** `certificate_structure.rs` verifies this signature
    // against this certificate's own SPKI; `check_signed_chain`
    // (`verify_cert.rs`) verifies each `SignedData` against the *issuer's*
    // SPKI and only then advances. A leaf can satisfy one and fail the other.
    //
    // The two roles get different seeds (`role_seed`), so the anchor's key is
    // a different key and this line cannot succeed by accident.
    // **The issuer's seed is derived from the issuer's NAME**, the one that
    // `server_for` mints the anchor with (`{dns_name}-anchor`): a seed of
    // the leaf's own name signs with a third key that is nowhere in the
    // chain.
    let issuer_key = if is_ca {
        own.clone()
    } else {
        SigningKey::from_bytes(&sha256(&role_seed(&anchor_name(dns_name), true)).into()).expect("a P-256 scalar")
    };
    let signature: Signature = issuer_key.sign(&tbs);
    eprintln!("BUILDER sig={} key-is-own={}", signature.to_der().as_bytes().len(), is_ca);
    // **The certificate is `SEQUENCE { tbs, algId, signatureValue }`, and
    // the SEQUENCE is a NEW header around all three.** The two outer fields
    // appended to `tbs` leave the TBS's length as the certificate's, and
    // webpki reports `BadEncoding`; `der_length_covers_the_certificate`
    // checks the declared length against the byte count.
    let cert = seq(&[tbs, seq(&[sig_alg]), tlv(0x03, &[&[0x00][..], signature.to_der().as_bytes()].concat())]);

    TestCert { der: cert, key_der: own.to_bytes().to_vec() }
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// **The declared length must cover the bytes.** A header of the wrong
/// structure (the TBS's own SEQUENCE header in place of the certificate's)
/// makes each field unreadable, and webpki reports `BadEncoding`, a name
/// that points at the certificate rather than at the length. A
/// hand-written encoder gets this check.
pub fn der_length_covers_the_certificate(der: &[u8]) {
    assert_eq!(der[0], 0x30, "a certificate is a SEQUENCE");
    let (declared, header) = read_len(der, 1);
    assert_eq!(
        header + declared,
        der.len(),
        "the certificate declares {declared} bytes of content and carries {}",
        der.len() - header
    );
}

/// `der` as a PEM certificate, base64 in lines of 64 characters, as
/// `--ca-file` and a system's tools read it.
pub(crate) fn pem(der: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in der.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in text.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}
