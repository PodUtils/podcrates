//! A P-256 private key as PKCS #8 (RFC 5208) around an ECPrivateKey (RFC
//! 5915), the form of `-----BEGIN PRIVATE KEY-----`. podnet's identity file
//! keeps the key of the device in this form (rcgen on ring writes it), so
//! that a file of podnet reads here, and a file of this crate reads in
//! podnet.

use super::{BIT_STRING, EC_P256_ALG, INTEGER, SEQUENCE, element, tlv};

/// The tag of an OCTET STRING.
pub const OCTET_STRING: u8 = 0x04;
/// The tag of the `[1]` publicKey of an ECPrivateKey.
const PUBLIC_KEY_TAG: u8 = 0xa1;

/// The PKCS #8 of the scalar `scalar` (32 bytes) and its SEC1 point
/// `point`: version 0, the algorithm of a P-256 key, and an ECPrivateKey of
/// version 1 with the public key, as ring writes one.
pub fn write(scalar: &[u8], point: &[u8]) -> Vec<u8> {
    let public = tlv(PUBLIC_KEY_TAG, &tlv(BIT_STRING, &[&[0u8][..], point].concat()));
    let ec = tlv(SEQUENCE, &[tlv(INTEGER, &[1]), tlv(OCTET_STRING, scalar), public].concat());
    tlv(SEQUENCE, &[tlv(INTEGER, &[0]), tlv(SEQUENCE, &EC_P256_ALG), tlv(OCTET_STRING, &ec)].concat())
}

/// The 32 bytes of the scalar of a PKCS #8 of a P-256 key; `None` for
/// another algorithm, another curve or a bad encoding. A version 2 (with a
/// public key after the private key) reads too.
pub fn read(der: &[u8]) -> Option<&[u8]> {
    let (SEQUENCE, info, _) = element(der)? else { return None };
    let (INTEGER, version, rest) = element(info)? else { return None };
    if version != [0] && version != [1] {
        return None;
    }
    let (SEQUENCE, algorithm, rest) = element(rest)? else { return None };
    if algorithm != EC_P256_ALG {
        return None;
    }
    let (OCTET_STRING, ec, _) = element(rest)? else { return None };
    let (SEQUENCE, ec, _) = element(ec)? else { return None };
    let (INTEGER, ec_version, rest) = element(ec)? else { return None };
    if ec_version != [1] {
        return None;
    }
    let (OCTET_STRING, scalar, _) = element(rest)? else { return None };
    (scalar.len() == 32).then_some(scalar)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PKCS #8 of ring's form (rcgen's `serialize_der`, the key of a
    /// podnet identity file) reads back, and the form that `write` gives is
    /// that form: 138 bytes.
    #[test]
    fn a_key_reads_back_in_ring_s_form() {
        let scalar = [7u8; 32];
        let point = [4u8; 65];
        let der = write(&scalar, &point);
        assert_eq!(der.len(), 138);
        assert_eq!(&der[..2], &[0x30, 0x81]);
        assert_eq!(read(&der), Some(&scalar[..]));
    }

    #[test]
    fn another_key_is_refused() {
        let der = write(&[7u8; 32], &[4u8; 65]);
        let mut other_curve = der.clone();
        // The last byte of the OID of the curve: prime256v1 to another.
        let at = der.windows(EC_P256_ALG.len()).position(|w| w == EC_P256_ALG).unwrap() + EC_P256_ALG.len() - 1;
        other_curve[at] ^= 1;
        assert_eq!(read(&other_curve), None);
        assert_eq!(read(&der[..der.len() - 1]), None, "one byte short");
        assert_eq!(read(&[]), None);
        let short = write(&[7u8; 31], &[4u8; 65]);
        assert_eq!(read(&short), None, "a scalar of 31 bytes");
    }
}
