//! The RFC 3161 timestamp of a bundle (`timestampVerificationData`): a
//! timestamp authority signed the time at which it saw the bundle's
//! signature. The token is a CMS SignedData (RFC 5652) of a TSTInfo; the
//! check reads its DER with `asn1-rs` (under x509-parser), takes the hash
//! of the signature from the TSTInfo, the digest of the TSTInfo from the
//! signed attributes, and the signature of the attributes by the key of the
//! authority's leaf, whose chain the caller gives. A certificate that the
//! token carries is not read: the chain is the caller's.

use x509_parser::asn1_rs::{Any, FromDer, Oid};
use x509_parser::prelude::*;

use super::chain;

const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const TST_INFO: &str = "1.2.840.113549.1.9.16.1.4";
const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
const SHA256: &str = "2.16.840.1.101.3.4.2.1";
const SHA384: &str = "2.16.840.1.101.3.4.2.2";
const SHA512: &str = "2.16.840.1.101.3.4.2.3";

/// The first byte of each DER element that the check reads.
const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const INTEGER: u8 = 0x02;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const EXPLICIT_0: u8 = 0xa0;
const IMPLICIT_1: u8 = 0xa1;

/// One element: its first byte (class, form and a tag number under 31),
/// its contents, and its whole encoding.
struct Element<'a> {
    id: u8,
    data: &'a [u8],
    whole: &'a [u8],
}

/// The elements of a constructed value, one after the other.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn next(&mut self, what: &str) -> Result<Element<'a>, String> {
        let (rest, any) = Any::from_der(self.0).map_err(|_| format!("the timestamp: {what} is not DER"))?;
        let header = &any.header;
        let tag = u8::try_from(header.tag().0).ok().filter(|t| *t < 31).ok_or(format!("the timestamp: {what}"))?;
        let id = (header.class() as u8) << 6 | u8::from(header.is_constructed()) << 5 | tag;
        let whole = &self.0[..self.0.len() - rest.len()];
        self.0 = rest;
        Ok(Element { id, data: any.data, whole })
    }

    /// The next element, which must start with `id`.
    fn take(&mut self, id: u8, what: &str) -> Result<Element<'a>, String> {
        let e = self.next(what)?;
        if e.id != id {
            return Err(format!("the timestamp: {what} is not of its type"));
        }
        Ok(e)
    }

    /// The next element when it starts with `id`.
    fn take_if(&mut self, id: u8) -> Option<Element<'a>> {
        let mut ahead = Reader(self.0);
        let e = ahead.next("").ok().filter(|e| e.id == id)?;
        self.0 = ahead.0;
        Some(e)
    }

    /// The text of the next element, an OID.
    fn oid(&mut self, what: &str) -> Result<String, String> {
        Ok(Oid::new(self.take(OID, what)?.data.into()).to_id_string())
    }

    /// The OID of the next element, an AlgorithmIdentifier.
    fn algorithm(&mut self, what: &str) -> Result<String, String> {
        Reader(self.take(SEQUENCE, what)?.data).oid(what)
    }
}

/// The digest of `bytes` by the algorithm of the OID `algorithm`.
fn digest(algorithm: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    use sha2::Digest;
    Ok(match algorithm {
        SHA256 => sha2::Sha256::digest(bytes).to_vec(),
        SHA384 => sha2::Sha384::digest(bytes).to_vec(),
        SHA512 => sha2::Sha512::digest(bytes).to_vec(),
        other => return Err(format!("the timestamp: the digest {other} is not known")),
    })
}

/// The time (seconds since the epoch) of the timestamp `der` (a
/// TimeStampResp, or the token alone) of `signed`, by the authority whose
/// chain, its leaf first, is `tsa`.
pub(super) fn check(der: &[u8], signed: &[u8], tsa: &[X509Certificate<'_>]) -> Result<i64, String> {
    let mut all = Reader(der);
    let response = all.take(SEQUENCE, "the response")?.data;
    if !all.0.is_empty() {
        return Err("the timestamp has bytes after its end".into());
    }
    let mut outer = Reader(response);
    let token = if let Some(status) = outer.take_if(SEQUENCE) {
        // PKIStatusInfo: granted (0) or granted with changes (1).
        if !matches!(Reader(status.data).take(INTEGER, "the status")?.data, [0] | [1]) {
            return Err("the timestamp: the authority did not grant it".into());
        }
        outer.take(SEQUENCE, "the token")?.data
    } else {
        // The token alone: a ContentInfo.
        response
    };
    let mut info = Reader(token);
    if info.oid("the content type")? != SIGNED_DATA {
        return Err("the timestamp is not a SignedData".into());
    }
    let mut sd = Reader(Reader(info.take(EXPLICIT_0, "the content")?.data).take(SEQUENCE, "the SignedData")?.data);
    sd.take(INTEGER, "the version")?;
    sd.take(SET, "the digest algorithms")?;
    let mut encap = Reader(sd.take(SEQUENCE, "the content")?.data);
    if encap.oid("the type of the content")? != TST_INFO {
        return Err("the timestamp holds no TSTInfo".into());
    }
    let tst = Reader(encap.take(EXPLICIT_0, "the TSTInfo")?.data).take(OCTET_STRING, "the TSTInfo")?.data;
    sd.take_if(EXPLICIT_0);
    sd.take_if(IMPLICIT_1);
    let mut signers = Reader(sd.take(SET, "the signers")?.data);
    let signer = signers.take(SEQUENCE, "the signer")?.data;
    if !signers.0.is_empty() {
        return Err("the timestamp has more than one signer".into());
    }
    let time = tst_info(tst, signed)?;
    signed_by(signer, tst, tsa, time)?;
    Ok(time)
}

/// The signer `signer` (a SignerInfo) signed the digest of `tst` with the
/// key of the leaf of `tsa`, and the chain held at `time`.
fn signed_by(signer: &[u8], tst: &[u8], tsa: &[X509Certificate<'_>], time: i64) -> Result<(), String> {
    let mut si = Reader(signer);
    si.take(INTEGER, "the signer's version")?;
    // The name of the signer: the check takes the key of the caller's leaf.
    si.next("the signer's name")?;
    let algorithm = si.algorithm("the signer's digest")?;
    let attributes = si.take(EXPLICIT_0, "the signed attributes")?;
    let signature_algorithm = match si.algorithm("the signature's algorithm")?.as_str() {
        EC_PUBLIC_KEY if algorithm == SHA384 => chain::ECDSA_SHA384.to_string(),
        EC_PUBLIC_KEY => chain::ECDSA_SHA256.to_string(),
        other => other.to_string(),
    };
    let signature = si.take(OCTET_STRING, "the signature")?.data;
    let (mut typed, mut digested) = (false, false);
    let mut each = Reader(attributes.data);
    while !each.0.is_empty() {
        let mut attribute = Reader(each.take(SEQUENCE, "a signed attribute")?.data);
        let kind = attribute.oid("a signed attribute")?;
        let mut values = Reader(attribute.take(SET, "a signed attribute")?.data);
        match kind.as_str() {
            CONTENT_TYPE => typed = values.oid("the content type")? == TST_INFO,
            MESSAGE_DIGEST => digested = values.take(OCTET_STRING, "the digest")?.data == digest(&algorithm, tst)?,
            _ => {}
        }
    }
    if !typed || !digested {
        return Err("the timestamp: the signed attributes do not name the TSTInfo".into());
    }
    let leaf = tsa.first().ok_or("no chain of a timestamp authority to check the timestamp by")?;
    // The signature is over the attributes as a SET OF (RFC 5652, 5.4).
    let mut signed = attributes.whole.to_vec();
    signed[0] = SET;
    chain::ecdsa(leaf.public_key(), &signature_algorithm, &signed, signature)
        .map_err(|why| format!("the timestamp's signature: {why}"))?;
    if !leaf.extended_key_usage().ok().flatten().is_some_and(|e| e.value.time_stamping) {
        return Err("the leaf of the timestamp authority is not for timestamps".into());
    }
    let mut cert = leaf;
    for issuer in &tsa[1..] {
        chain::signed_by(cert, issuer, time)?;
        cert = issuer;
    }
    chain::signed_by(cert, cert, time)
}

/// The time of the TSTInfo `tst`, which must hold the digest of `signed`.
fn tst_info(tst: &[u8], signed: &[u8]) -> Result<i64, String> {
    let mut t = Reader(Reader(tst).take(SEQUENCE, "the TSTInfo")?.data);
    if t.take(INTEGER, "the TSTInfo's version")?.data != [1] {
        return Err("the timestamp: the TSTInfo is not of version 1".into());
    }
    t.oid("the policy")?;
    let mut imprint = Reader(t.take(SEQUENCE, "the imprint")?.data);
    let algorithm = imprint.algorithm("the imprint's algorithm")?;
    if imprint.take(OCTET_STRING, "the imprint")?.data != digest(&algorithm, signed)? {
        return Err("the timestamp is of another signature".into());
    }
    t.take(INTEGER, "the serial")?;
    let at = t.next("the time")?;
    let (_, time) = ASN1Time::from_der(at.whole).map_err(|_| "the timestamp: its time is not a time".to_string())?;
    Ok(time.timestamp())
}
