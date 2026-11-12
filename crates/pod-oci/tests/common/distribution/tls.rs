//! The certificates of the target `distribution`, minted in pure Rust by
//! the encoder of the tests of `pod-tls`: one root, and one leaf for
//! 127.0.0.1 under it. The leaf serves the TLS of the registry and of the
//! token server, and its key signs each token: the registry finds the leaf
//! in the `x5c` header of a token and checks it against the root.

use std::sync::Arc;

use p256::ecdsa::SigningKey;

#[allow(dead_code, unused_imports)]
mod cert {
    include!("../../../../pod-tls/tests/common/cert_encoding.rs");
}

use cert::{oid, seq, tlv};

/// The root, the leaf and the key of the leaf.
pub struct Pki {
    pub anchor_der: Vec<u8>,
    pub leaf_der: Vec<u8>,
    pub key: SigningKey,
}

impl Pki {
    pub fn new() -> Pki {
        let anchor = cert::self_signed_inner(&cert::anchor_name(super::AT), true);
        let leaf = cert::self_signed_inner(super::AT, false);
        let key = SigningKey::from_slice(&leaf.key_der).expect("a P-256 scalar");
        Pki { anchor_der: anchor.der, leaf_der: leaf.der, key }
    }

    pub fn anchor_pem(&self) -> String {
        pem("CERTIFICATE", &self.anchor_der)
    }

    pub fn leaf_pem(&self) -> String {
        pem("CERTIFICATE", &self.leaf_der)
    }

    /// The key of the leaf as RFC 5915's ECPrivateKey, in DER: Go and
    /// rustls both read it.
    pub fn key_sec1(&self) -> Vec<u8> {
        let point = self.key.verifying_key().to_sec1_point(false);
        let mut body = vec![0x02, 0x01, 0x01, 0x04, 0x20];
        body.extend_from_slice(&self.key.to_bytes());
        body.extend_from_slice(&tlv(0xa0, &oid(&[1, 2, 840, 10045, 3, 1, 7])));
        body.extend_from_slice(&tlv(0xa1, &tlv(0x03, &[&[0x00][..], point.as_bytes()].concat())));
        seq(&[body])
    }

    pub fn key_pem(&self) -> String {
        pem("EC PRIVATE KEY", &self.key_sec1())
    }

    /// The roots of a client of the test: the root alone.
    pub fn roots(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(rustls_pki_types::CertificateDer::from(self.anchor_der.clone())).expect("the root of the test");
        roots
    }

    /// A server that presents the leaf, on ring.
    pub fn server_config(&self) -> Arc<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let chain = vec![rustls_pki_types::CertificateDer::from(self.leaf_der.clone())];
        let key = rustls_pki_types::PrivateKeyDer::from(rustls_pki_types::PrivateSec1KeyDer::from(self.key_sec1()));
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the versions of ring")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("the server presents the leaf");
        Arc::new(config)
    }

    /// A client that trusts the root alone, on ring.
    pub fn client_config(&self) -> Arc<rustls::ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the versions of ring")
            .with_root_certificates(self.roots())
            .with_no_client_auth();
        Arc::new(config)
    }
}

/// PEM of `der` with the label `label`.
pub fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let text = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in text.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).unwrap());
        out.push('\n');
    }
    out + &format!("-----END {label}-----\n")
}
