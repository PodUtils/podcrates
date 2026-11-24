//! The certificates of the tests of the carriers over TLS to a server of
//! another party (the mock mail servers, the stand-in resolver of DNS over
//! HTTPS): a CA and a leaf made in this process, the acceptor of a server,
//! and the client configuration that trusts the CA.

use std::sync::Arc;

use pod_tls::server::TlsAcceptor;
use rustls::crypto::CryptoProvider;

/// The certificates of the tests, made in this process: a CA, and a leaf
/// for `localhost` and `127.0.0.1` that it signs. They authenticate nothing
/// that matters, and no key of a test is in the repository.
pub struct TestCertificate {
    pub leaf: String,
    pub key: String,
    pub ca: String,
}

/// The certificates of this process, made once.
pub fn certificate() -> &'static TestCertificate {
    use std::sync::OnceLock;

    use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
    static CERTIFICATE: OnceLock<TestCertificate> = OnceLock::new();
    CERTIFICATE.get_or_init(|| {
        let ca_key = KeyPair::generate().expect("a key of the CA of the tests");
        let mut ca_params = CertificateParams::new(vec!["podlink test CA".to_string()]).expect("the CA parameters");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).expect("the CA of the tests");
        let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
        let leaf_key = KeyPair::generate().expect("a key of the leaf of the tests");
        let mut leaf_params = CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("the leaf parameters");
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf = leaf_params.signed_by(&leaf_key, &issuer).expect("the leaf of the tests");
        TestCertificate { leaf: leaf.pem(), key: leaf_key.serialize_pem(), ca: ca.pem() }
    })
}

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The acceptor of the mock servers.
pub fn acceptor() -> TlsAcceptor {
    acceptor_offering(&[])
}

/// The acceptor of a server that offers the ALPN of `alpn`.
pub fn acceptor_offering(alpn: &[&[u8]]) -> TlsAcceptor {
    let certificate = certificate();
    pod_tls::server::acceptor(provider(), certificate.leaf.as_bytes(), certificate.key.as_bytes(), alpn)
        .expect("the certificate of the tests")
}

/// The client configuration that trusts the certificate of the tests.
pub fn client_config() -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for certificate in pod_tls::server::certificates(certificate().ca.as_bytes()).expect("the CA of the tests") {
        roots.add(certificate).expect("the root of the tests");
    }
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("TLS 1.3 and 1.2")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}
