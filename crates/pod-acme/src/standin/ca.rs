//! The test CA of the stand-in: made at its start, never kept. It signs the
//! certificate of the stand-in's HTTPS and each certificate that an order
//! issues, after the checks of a CSR that a CA makes (its signature, its
//! names, its key).

use std::sync::Arc;

use p256::ecdsa::signature::Verifier as _;
use rustls::crypto::CryptoProvider;
use sha2::Digest as _;
use x509_parser::prelude::*;

use crate::certs::{self, Key};

/// The OID of ecdsa-with-SHA256 (RFC 5758, section 3.2).
const ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
/// The OIDs of id-ecPublicKey and of the curve P-256 (RFC 5480, section 2.1.1).
const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
const P256: &str = "1.2.840.10045.3.1.7";

/// The CA of the stand-in.
pub struct Ca {
    /// The certificate of the CA, as PEM: the one root that a client of the
    /// stand-in trusts.
    pub pem: String,
    issuer: rcgen::Issuer<'static, Key>,
}

/// The public key of a CSR, as rcgen signs it into a certificate.
struct Requested(Vec<u8>);

impl rcgen::PublicKeyData for Requested {
    fn der_bytes(&self) -> &[u8] {
        &self.0
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

impl Ca {
    pub fn new() -> Result<Ca, String> {
        let key = Key::generate()?;
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
            rcgen::KeyUsagePurpose::DigitalSignature,
        ];
        params.distinguished_name.push(rcgen::DnType::CommonName, "pod-acme stand-in CA");
        dated(&mut params, &key, 30)?;
        let cert = params.self_signed(&key).map_err(|e| format!("the test CA: {e}"))?;
        Ok(Ca { pem: certs::pem("CERTIFICATE", cert.der()), issuer: rcgen::Issuer::new(params, key) })
    }

    /// The TLS acceptor of the stand-in's HTTPS: a leaf for `127.0.0.1` and
    /// `localhost`, signed by the CA.
    pub fn acceptor(&self, provider: Arc<CryptoProvider>) -> Result<pod_tls::server::TlsAcceptor, String> {
        let key = Key::generate()?;
        let names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
        let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        dated(&mut params, &key, 30)?;
        let cert = params.signed_by(&key, &self.issuer).map_err(|e| format!("the stand-in's leaf: {e}"))?;
        let chain = certs::pem("CERTIFICATE", cert.der());
        pod_tls::server::acceptor(provider, chain.as_bytes(), key.pem().as_bytes(), &[pod_tls::server::HTTP1])
            .map_err(|e| e.to_string())
    }

    /// The chain of a certificate for `names` alone from the CSR `der`, for
    /// `days`: the leaf, then the CA. A CSR whose signature does not
    /// verify, whose key is not P-256 or is `account` (the point of the
    /// account's key), or whose names are not `names`, is refused with the
    /// words of a `badCSR`.
    pub fn issue(&self, der: &[u8], names: &[String], account: &[u8], days: i64) -> Result<String, String> {
        let (rest, csr) =
            X509CertificationRequest::from_der(der).map_err(|e| format!("the CSR cannot be read: {e}"))?;
        if !rest.is_empty() {
            return Err("the CSR has bytes after its end".into());
        }
        let info = &csr.certification_request_info;
        let spki = &info.subject_pki;
        let curve = spki.algorithm.parameters.as_ref().and_then(|p| p.as_oid().ok()).map(|o| o.to_id_string());
        if spki.algorithm.algorithm.to_id_string() != EC_PUBLIC_KEY || curve.as_deref() != Some(P256) {
            return Err("the key of the CSR is not a P-256 key: the stand-in takes P-256 alone".into());
        }
        let point = spki.subject_public_key.data.to_vec();
        if point == account {
            return Err("the key of the CSR is the key of the account".into());
        }
        if csr.signature_algorithm.algorithm.to_id_string() != ECDSA_SHA256 {
            return Err("the CSR is not signed with ecdsa-with-SHA256".into());
        }
        let verifying =
            p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).map_err(|_| "the key of the CSR is no P-256 point")?;
        let signature = p256::ecdsa::Signature::from_der(&csr.signature_value.data)
            .map_err(|_| "the signature of the CSR is not DER")?;
        verifying.verify(info.raw, &signature).map_err(|_| "the signature of the CSR does not verify")?;
        let asked = csr_names(&csr);
        let mut want: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        want.sort();
        if asked != want {
            return Err(format!("the CSR names {asked:?}, and the order names {want:?}"));
        }
        let mut params = rcgen::CertificateParams::new(want).map_err(|e| e.to_string())?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let requested = Requested(point);
        params.key_identifier_method = rcgen::KeyIdMethod::PreSpecified(key_id(&requested.0));
        let now = crate::order::now_secs();
        life(&mut params, now - 86_400, now + days * 86_400);
        params.serial_number = Some(serial()?);
        let cert = params.signed_by(&requested, &self.issuer).map_err(|e| format!("the certificate: {e}"))?;
        Ok(format!("{}{}", certs::pem("CERTIFICATE", cert.der()), self.pem))
    }
}

/// The DNS names of a CSR, in lower case and in order: of its
/// subjectAltName and of its common name.
fn csr_names(csr: &X509CertificationRequest<'_>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for extension in csr.requested_extensions().into_iter().flatten() {
        if let ParsedExtension::SubjectAlternativeName(san) = extension {
            for name in &san.general_names {
                match name {
                    GeneralName::DNSName(d) => names.push(d.to_ascii_lowercase()),
                    other => names.push(format!("{other:?}")),
                }
            }
        }
    }
    for cn in csr.certification_request_info.subject.iter_common_name() {
        if let Ok(text) = cn.as_str() {
            names.push(text.to_ascii_lowercase());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The life of a certificate of `key`: from a day before now, for `days`;
/// its key identifier from the key, as rcgen with no crypto feature makes
/// none.
fn dated(params: &mut rcgen::CertificateParams, key: &Key, days: i64) -> Result<(), String> {
    let now = crate::order::now_secs();
    life(params, now - 86_400, now + days * 86_400);
    params.key_identifier_method = rcgen::KeyIdMethod::PreSpecified(key_id(key.public()));
    params.serial_number = Some(serial()?);
    Ok(())
}

/// The first 20 bytes of the SHA-256 of a public point.
fn key_id(point: &[u8]) -> Vec<u8> {
    sha2::Sha256::digest(point)[..20].to_vec()
}

/// A life from the day of `from` to the day of `to`, each at midnight UTC,
/// as rcgen takes it.
fn life(params: &mut rcgen::CertificateParams, from: i64, to: i64) {
    let day = |secs: i64| {
        let (y, m, d) = pod_core::clock::civil_from_days(secs.div_euclid(86_400));
        rcgen::date_time_ymd(y as i32, m as u8, d as u8)
    };
    params.not_before = day(from);
    params.not_after = day(to);
}

/// A serial number of 16 random bytes, the first bit clear.
fn serial() -> Result<rcgen::SerialNumber, String> {
    use pod_core::entropy::Entropy as _;
    let mut bytes = [0u8; 16];
    pod_core::entropy::OsEntropy.fill(&mut bytes).map_err(|e| format!("no serial number: {e}"))?;
    bytes[0] &= 0x7f;
    Ok(rcgen::SerialNumber::from_slice(&bytes))
}
