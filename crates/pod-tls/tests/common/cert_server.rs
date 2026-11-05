// **A TLS server that presents the certificate `cert_encoding.rs` mints.**
// Included by `cert_for_test.rs`; not a test target of its own.
//
// **This is the rustls side and nothing else.** The bytes come from the
// encoding half and the client configuration under test comes from
// `pod_tls`; what is here is the part between them. The server runs on
// ring, a TLS stack that the crate did not write, whatever provider the
// client under test has.

use std::sync::Arc;

use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;

use super::encoding::{TestCert, der_length_covers_the_certificate, read_len, self_signed_inner};

/// **A `TlsAcceptor` serving `dns_name`, and a root store that trusts it.**
///
/// Every step can fail, so this returns a `Result`: a test helper that can
/// fail does not hide it.
pub fn server_for(dns_name: &str) -> Result<(tokio_rustls::TlsAcceptor, rustls::RootCertStore), String> {
    server_for_groups(dns_name, None)
}

/// **[`server_for`], with `groups` alone for the key exchange** when given:
/// a server that takes none of the client's first offer answers it with a
/// HelloRetryRequest.
pub fn server_for_groups(
    dns_name: &str,
    groups: Option<Vec<&'static dyn rustls::crypto::SupportedKxGroup>>,
) -> Result<(tokio_rustls::TlsAcceptor, rustls::RootCertStore), String> {
    // **TWO certificates: an anchor and a leaf.** One self-signed certificate
    // as both the trust anchor and the server's certificate is refused by
    // webpki with `OtherError(CaUsedAsEndEntity)`: an anchor is a CA, and a
    // server may not present a CA as its end-entity certificate.
    let anchor = self_signed_inner(&format!("{dns_name}-anchor"), true);
    let leaf = self_signed_inner(dns_name, false);

    der_length_covers_the_certificate(&anchor.der);
    der_length_covers_the_certificate(&leaf.der);

    let mut roots = rustls::RootCertStore::empty();
    // **`?`, not `.expect`.** A trust anchor that fails to load is a fact
    // that the caller must be able to fail on, not a panic of a helper.
    roots
        .add(rustls_pki_types::CertificateDer::from(anchor.der.clone()))
        .map_err(|e| format!("the self-signed CA is not a usable anchor: {e}"))?;

    // The server presents the leaf alone. The anchor reaches the client
    // inside the trust store, which is exactly what a deployment's CA bundle
    // does — the same arrangement `pod_tls::roots_from_bundle` builds.
    let chain = vec![rustls_pki_types::CertificateDer::from(leaf.der.clone())];
    let key = key_for(&leaf)?;

    // The server's key goes through the test's own `KeyProvider`, which
    // reads the SEC1 key that `key_for` writes.
    let mut provider = rustls::crypto::ring::default_provider();
    provider.key_provider = &TestKeyProvider;
    if let Some(groups) = groups {
        provider.kx_groups = groups;
    }
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS 1.3 is not offered: {e}"))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| format!("the server cannot present its own certificate: {e}"))?;

    Ok((tokio_rustls::TlsAcceptor::from(Arc::new(server_config)), roots))
}

/// **The key is the LEAF's own key**, because the leaf is the certificate
/// being presented. The anchor's key never leaves `server_for`.
///
/// **A real SEC1 `ECPrivateKey`, assembled by hand.** `PrivateSec1KeyDer`
/// gives its bytes *verbatim* from `secret_sec1_der()`, with no unwrapping and
/// no re-encoding: a bare 32-byte scalar in its place is raw bytes where a DER
/// structure is declared. That fault shows as `BadSignature` in the server's
/// `CertificateVerify` (a `DigitallySignedStruct` over the transcript, not a
/// certificate signature), with the same error name as a chain that does not
/// verify.
fn key_for(leaf: &TestCert) -> Result<rustls_pki_types::PrivateKeyDer<'static>, String> {
    use super::encoding::{oid, seq, tlv};

    let signing =
        SigningKey::from_slice(&leaf.key_der[..]).map_err(|_| "the test key is not a valid P-256 scalar".to_string())?;
    let point = signing.verifying_key().to_sec1_point(false);
    let point = point.as_bytes();

    // RFC 5915 ECPrivateKey ::= SEQUENCE { version, privateKey, [0] parameters,
    // [1] publicKey }. The optional fields are what make it a curve-agnostic
    // structure rather than a bare scalar.
    let mut body = vec![0x02, 0x01, 0x01]; // INTEGER 1
    body.extend_from_slice(&[0x04, 0x20]); // OCTET STRING, 32 bytes
    body.extend_from_slice(&leaf.key_der);
    body.extend_from_slice(&tlv(0xa0, &oid(&[1, 2, 840, 10045, 3, 1, 7])));
    body.extend_from_slice(&tlv(0xa1, &tlv(0x03, &[&[0x00][..], point].concat())));

    Ok(rustls_pki_types::PrivateKeyDer::from(rustls_pki_types::PrivateSec1KeyDer::from(seq(&[body]))))
}

/// **A `KeyProvider` for the test's server side only.** The pure provider's
/// is `NoClientKeys`, which refuses every key; the server of a test has its
/// own loader, so that no provider under test is changed for it.
#[derive(Debug)]
struct TestKeyProvider;

impl rustls::crypto::KeyProvider for TestKeyProvider {
    fn load_private_key(
        &self,
        key_der: rustls_pki_types::PrivateKeyDer<'static>,
    ) -> Result<Arc<dyn rustls::sign::SigningKey>, rustls::Error> {
        // `with_single_cert` asks the provider for the key, so the loader is
        // real, not a stub.
        //
        // **It parses the SEC1 structure rather than guessing at an offset.**
        // The last 32 bytes of the container are the scalar only when nothing
        // follows it. One walk, from the front, with the DER length deciding
        // where the field ends.
        let contents = read_sequence(key_der.secret_der(), 0)?;
        let fields = tlv_fields(&contents, "an ECPrivateKey")?;
        // Each field is a *value*, not a TLV: the INTEGER's content is `01`,
        // not `02 01 01`.
        if fields.len() < 2 || fields[0] != [0x01] {
            return Err(rustls::Error::General("an ECPrivateKey starts with INTEGER 1".into()));
        }
        // The tag is checked by the *length*, not by a prefix on the value:
        // `tlv_fields` strips headers, so the scalar is all 32 bytes.
        let secret: &[u8] = &fields[1];
        let bytes: [u8; 32] =
            <[u8; 32]>::try_from(secret).map_err(|_| rustls::Error::General("the test key is not 32 bytes".into()))?;
        let signing = SigningKey::from_bytes(&bytes.into())
            .map_err(|_| rustls::Error::General("the test key is not a valid scalar".into()))?;
        Ok(Arc::new(TestSigningKey { signing }))
    }
}

#[derive(Debug)]
struct TestSigningKey {
    signing: p256::ecdsa::SigningKey,
}

impl rustls::sign::SigningKey for TestSigningKey {
    fn choose_scheme(&self, offered: &[rustls::SignatureScheme]) -> Option<Box<dyn rustls::sign::Signer>> {
        // **A scheme that this key can produce, and no other.** rustls gives
        // `choose_scheme` the client's `signature_algorithms` in wire order,
        // and the pure provider puts P-384 first; a signer that named the
        // client's first scheme would make the client verify a P-256
        // signature with `ECDSA_P384_SHA384`, which refuses a P-256 key, and
        // the handshake would end in `BadSignature`.
        //
        // `SigningKey::choose_scheme` promises that the signer it returns is
        // for the algorithm that `Signer::scheme` names. **This key is P-256
        // and this certificate is on P-256, so P-384 is not an option here**,
        // however the peer prefers it.
        let offered = offered.iter().find(|s| **s == rustls::SignatureScheme::ECDSA_NISTP256_SHA256);
        offered.map(|scheme| {
            Box::new(TestSigner { signing: self.signing.clone(), scheme: *scheme }) as Box<dyn rustls::sign::Signer>
        })
    }

    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
}

#[derive(Debug)]
struct TestSigner {
    signing: p256::ecdsa::SigningKey,
    /// The scheme this signer actually produces, so `scheme()` cannot disagree
    /// with the key.
    scheme: rustls::SignatureScheme,
}

impl rustls::sign::Signer for TestSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        // **Sign the message and let the scheme hash it.** rustls gives
        // `Signer::sign` an unhashed transcript and the verifier hashes the
        // same bytes once, so both sides agree. A pre-hashed message, which
        // `p256` hashes again, gives a signature that nothing can verify.
        let sig: p256::ecdsa::Signature = self.signing.sign(message);
        Ok(sig.to_der().as_bytes().to_vec())
    }

    fn scheme(&self) -> rustls::SignatureScheme {
        self.scheme
    }
}

/// The content of the SEQUENCE at `der[0]`, and every TLV inside it.
fn read_sequence(der: &[u8], at: usize) -> Result<Vec<u8>, rustls::Error> {
    match der.get(at) {
        Some(0x30) => {}
        other => return Err(rustls::Error::General(format!("expected a SEQUENCE at offset {at}, found {other:02x?}"))),
    }
    let (len, start) = read_len(der, at + 1);
    if start + len > der.len() {
        return Err(rustls::Error::General("the private key's declared length overruns the container".into()));
    }
    Ok(der[start..start + len].to_vec())
}

/// Split a concatenation of TLVs into each value's content.
fn tlv_fields(contents: &[u8], what: &str) -> Result<Vec<Vec<u8>>, rustls::Error> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < contents.len() {
        let (len, start) = read_len(contents, at + 1);
        let end = start + len;
        if end > contents.len() {
            return Err(rustls::Error::General(format!("{what} overruns")));
        }
        out.push(contents[start..end].to_vec());
        at = end;
    }
    Ok(out)
}
