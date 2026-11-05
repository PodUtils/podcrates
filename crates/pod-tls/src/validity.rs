//! The end of the validity (notAfter) of each certificate of a PEM text, so
//! that a consumer can give the first day when a root that it carries
//! stops (podnet: the edge roots of Cloudflare). A small DER reader of this
//! crate reads the dates; it checks no signature.

use crate::bundle;

/// The notAfter dates (`YYYY-MM-DD`) of the certificates of the PEM text
/// `pem`, in the order of the text. A text with no `CERTIFICATE` block gives
/// none.
pub fn not_after_dates(pem: &[u8]) -> Result<Vec<String>, String> {
    let certs = bundle::pem_certificates(pem).map_err(|why| format!("parsing the PEM: {why}"))?;
    certs.iter().map(|cert| not_after(cert.as_ref()).map_err(|why| format!("reading the validity: {why}"))).collect()
}

/// One DER element: its tag, its value and the bytes after it.
fn tlv(data: &[u8]) -> Result<(u8, &[u8], &[u8]), String> {
    let (&tag, rest) = data.split_first().ok_or("no tag")?;
    let (&first, rest) = rest.split_first().ok_or("no length")?;
    let (len, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return Err("bad length".into());
        }
        let len = rest[..n].iter().fold(0usize, |a, b| (a << 8) | *b as usize);
        (len, &rest[n..])
    };
    if rest.len() < len {
        return Err("short value".into());
    }
    Ok((tag, &rest[..len], &rest[len..]))
}

/// The notAfter of a DER certificate: Certificate -> TBSCertificate ->
/// (version, serial, signature, issuer, validity).
fn not_after(der: &[u8]) -> Result<String, String> {
    let (_, cert, _) = tlv(der)?;
    let (_, tbs, _) = tlv(cert)?;
    let mut rest = tbs;
    let (tag, _, after) = tlv(rest)?;
    if tag == 0xa0 {
        rest = after; // the explicit version
    }
    for _ in 0..3 {
        rest = tlv(rest)?.2; // serial, signature, issuer
    }
    let (_, validity, _) = tlv(rest)?;
    let (_, _, after_start) = tlv(validity)?;
    let (tag, end, _) = tlv(after_start)?;
    let end = std::str::from_utf8(end).map_err(|_| "the time is not text")?;
    // A time too short for its form is an error, never a slice out of range.
    let bad = || format!("the time {end:?} is not UTCTime or GeneralizedTime");
    let (year, rest) = match tag {
        0x17 => {
            let yy: u32 = end.get(..2).and_then(|y| y.parse().ok()).ok_or_else(bad)?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &end[2..])
        }
        0x18 => (end.get(..4).and_then(|y| y.parse().ok()).ok_or_else(bad)?, &end[4..]),
        _ => return Err(format!("the time has tag {tag:#x}")),
    };
    let (month, day) = (rest.get(..2).ok_or_else(bad)?, rest.get(2..4).ok_or_else(bad)?);
    if !(month.bytes().chain(day.bytes())).all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    Ok(format!("{year:04}-{month}-{day}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DigiCert TLS ECC P384 Root G5: notAfter 460114235959Z (UTCTime).
    const DIGICERT_ECC_ROOT_G5: &str = "-----BEGIN CERTIFICATE-----
MIICGTCCAZ+gAwIBAgIQCeCTZaz32ci5PhwLBCou8zAKBggqhkjOPQQDAzBOMQsw
CQYDVQQGEwJVUzEXMBUGA1UEChMORGlnaUNlcnQsIEluYy4xJjAkBgNVBAMTHURp
Z2lDZXJ0IFRMUyBFQ0MgUDM4NCBSb290IEc1MB4XDTIxMDExNTAwMDAwMFoXDTQ2
MDExNDIzNTk1OVowTjELMAkGA1UEBhMCVVMxFzAVBgNVBAoTDkRpZ2lDZXJ0LCBJ
bmMuMSYwJAYDVQQDEx1EaWdpQ2VydCBUTFMgRUNDIFAzODQgUm9vdCBHNTB2MBAG
ByqGSM49AgEGBSuBBAAiA2IABMFEoc8Rl1Ca3iOCNQfN0MsYndLxf3c1TzvdlHJS
7cI7+Oz6e2tYIOyZrsn8aLN1udsJ7MgT9U7GCh1mMEy7H0cKPGEQQil8pQgO4CLp
0zVozptjn4S1mU1YoI71VOeVyaNCMEAwHQYDVR0OBBYEFMFRRVBZqz7nLFr6ICIS
B4CIfBFqMA4GA1UdDwEB/wQEAwIBhjAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49
BAMDA2gAMGUCMQCJao1H5+z8blUD2WdsJk6Dxv3J+ysTvLd6jLRl0mlpYxNjOyZQ
LgGheQaRnUi/wr4CMEfDFXuxoJGZSZOoPHzoRgaLLPIxAJSdYsiJvRmEFOml+wG4
DXZDjC5Ty3zfDBeWUA==
-----END CERTIFICATE-----
";

    fn element(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match value.len() {
            n if n < 0x80 => out.push(n as u8),
            n if n < 0x100 => out.extend([0x81, n as u8]),
            n => out.extend([0x82, (n >> 8) as u8, n as u8]),
        }
        out.extend_from_slice(value);
        out
    }

    /// The DER of a certificate whose notAfter is `value` with `tag`: with
    /// the explicit version (v3), or with none (v1).
    fn certificate(versioned: bool, tag: u8, value: &[u8]) -> Vec<u8> {
        let mut tbs = Vec::new();
        if versioned {
            tbs.extend(element(0xa0, &element(0x02, &[2])));
        }
        tbs.extend(element(0x02, &[1])); // serial
        tbs.extend(element(0x30, &element(0x06, &[0x2a]))); // signature
        tbs.extend(element(0x30, &[])); // issuer
        tbs.extend(element(0x30, &[element(0x17, b"200101000000Z"), element(tag, value)].concat()));
        element(0x30, &element(0x30, &tbs))
    }

    fn pem(der: &[u8]) -> String {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        format!("-----BEGIN CERTIFICATE-----\n{b64}\n-----END CERTIFICATE-----\n")
    }

    /// The notAfter of each certificate, in the order of the text: UTCTime
    /// with a year from 1950 to 2049, GeneralizedTime, with and with no
    /// explicit version.
    #[test]
    fn the_not_after_of_each_certificate() {
        assert_eq!(not_after_dates(DIGICERT_ECC_ROOT_G5.as_bytes()).unwrap(), vec!["2046-01-14"]);
        let text = [
            pem(&certificate(true, 0x18, b"20510101000000Z")),
            DIGICERT_ECC_ROOT_G5.to_string(),
            pem(&certificate(false, 0x17, b"991231235959Z")),
        ]
        .concat();
        assert_eq!(not_after_dates(text.as_bytes()).unwrap(), vec!["2051-01-01", "2046-01-14", "1999-12-31"]);
        assert_eq!(not_after_dates(b"no certificate here").unwrap(), Vec::<String>::new());
    }

    /// A text that is not PEM, and a certificate that is not DER or whose
    /// time is short or of another tag, is an error, never a panic.
    #[test]
    fn a_certificate_that_cannot_be_read_is_an_error() {
        let err = not_after_dates(b"-----BEGIN CERTIFICATE-----\nMAA").unwrap_err();
        assert!(err.starts_with("parsing the PEM: "), "{err}");
        for (der, why) in [
            (vec![0x30, 0x05, 0x30], "short value"),
            (vec![0x30, 0x02, 0x30, 0x80], "bad length"),
            (certificate(true, 0x05, b""), "the time has tag 0x5"),
            (certificate(true, 0x17, b"9"), "is not UTCTime or GeneralizedTime"),
            (certificate(true, 0x18, b"2051"), "is not UTCTime or GeneralizedTime"),
            (certificate(true, 0x17, b"99\xc3\xa9\xc3\xa9Z"), "is not UTCTime or GeneralizedTime"),
        ] {
            let err = not_after_dates(pem(&der).as_bytes()).unwrap_err();
            assert!(err.starts_with("reading the validity: ") && err.contains(why), "{err}");
        }
    }
}
