//! The cipher suites that the pure provider offers, and the
//! `CryptoProvider` that they form.
//!
//! **Every suite here has completed a handshake with a real peer.** A
//! suite that has not is worse than absent: the peer can select it, and the
//! failure arrives in the record layer rather than during negotiation.
//!
//! **The TLS 1.2 suites come after the TLS 1.3 ones**, for the
//! proxies that intercept TLS and speak only 1.2. Each completed a handshake
//! and moved data with OpenSSL's `s_server` (`tests/tls12.rs`). A server
//! that speaks 1.3 still gets 1.3: rustls offers both versions in one
//! ClientHello and checks the downgrade value of RFC 8446 section 4.1.3, so a
//! fall to 1.2 cannot be forced.

use rustls::crypto::CryptoProvider;
use rustls::{CipherSuite, SupportedCipherSuite};

use super::aead::{AeadId, PureAead};
use super::hash::HashAlgorithmId;
use super::hkdf::PureHkdf;
use super::kx::{SECP256R1, X25519};
use super::random::OsRandom;
use super::sign::{NoClientKeys, signature_algorithms};
use super::tls12;

/// `confidentiality_limit` is `2^24` for AES-GCM, the value rustls's own
/// reference providers use, cited there to `draft-irtf-aead-limits-08`. A
/// larger number would keep encrypting past the point where the bound analysis
/// holds.
pub static TLS13_AES_256_GCM_SHA384: SupportedCipherSuite = SupportedCipherSuite::Tls13(&rustls::Tls13CipherSuite {
    common: rustls::CipherSuiteCommon {
        suite: CipherSuite::TLS13_AES_256_GCM_SHA384,
        hash_provider: super::hash::SHA384,
        confidentiality_limit: 1 << 24,
    },
    hkdf_provider: &PureHkdf(HashAlgorithmId::Sha384),
    aead_alg: &PureAead(AeadId::Aes256Gcm),
    // QUIC (the quick tunnel's): its own keys, from `quic.rs`, and
    // checked against RFC 9001's vectors (`tests/quic_keys.rs`).
    quic: Some(&super::quic::AES_256_GCM),
});

pub static TLS13_AES_128_GCM_SHA256: SupportedCipherSuite = SupportedCipherSuite::Tls13(&rustls::Tls13CipherSuite {
    common: rustls::CipherSuiteCommon {
        suite: CipherSuite::TLS13_AES_128_GCM_SHA256,
        hash_provider: super::hash::SHA256,
        confidentiality_limit: 1 << 24,
    },
    hkdf_provider: &PureHkdf(HashAlgorithmId::Sha256),
    aead_alg: &PureAead(AeadId::Aes128Gcm),
    // Also the keys of QUIC's Initial packets, whatever suite follows.
    quic: Some(&super::quic::AES_128_GCM),
});

/// **Preference order is this order**, and the first entry is also the
/// default key share in the `ClientHello`. AES-256-GCM first because that is
/// what the live relay has been measured negotiating.
static ALL_CIPHER_SUITES: &[SupportedCipherSuite] = &[
    TLS13_AES_256_GCM_SHA384,
    TLS13_AES_128_GCM_SHA256,
    tls12::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    tls12::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    tls12::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
    // An RSA suite only where RSA signatures verify (feature `rsa`): a
    // server that chose one would sign its key exchange with RSA.
    #[cfg(feature = "rsa")]
    tls12::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    #[cfg(feature = "rsa")]
    tls12::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    #[cfg(feature = "rsa")]
    tls12::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
];

static ALL_KX_GROUPS: &[&'static dyn rustls::crypto::SupportedKxGroup] = &[X25519, SECP256R1];

/// **This is the whole of the provider.** Five fields, all of them supplied
/// by this crate. The binary gives it to each configuration
/// (`Arc::new(pod_tls::crypto::provider())`); nothing installs it as the
/// process default.
pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: ALL_CIPHER_SUITES.to_vec(),
        kx_groups: ALL_KX_GROUPS.to_vec(),
        signature_verification_algorithms: signature_algorithms(),
        secure_random: &OsRandom,
        key_provider: &NoClientKeys,
    }
}

/// **Every `SupportedKxGroup` in the provider must have a matching suite**,
/// and rustls documents that as a validity requirement. Asserting it here
/// rather than discovering it as a handshake failure is the difference between
/// a provider that is checked and one that is merely built.
pub fn self_check() -> Result<(), String> {
    check(&provider())
}

fn check(p: &CryptoProvider) -> Result<(), String> {
    if p.cipher_suites.is_empty() {
        return Err("the provider offers no cipher suites".into());
    }
    if p.kx_groups.is_empty() {
        return Err("the provider offers no key exchange groups".into());
    }
    if p.signature_verification_algorithms.all.is_empty() {
        return Err("the provider offers no signature algorithms".into());
    }
    // Every suite must name a hash, and that hash must be one of the two the
    // HKDF provider, and the HMAC under the TLS 1.2 PRF, know how to run.
    let known = |h: rustls::crypto::hash::HashAlgorithm| {
        matches!(h, rustls::crypto::hash::HashAlgorithm::SHA256 | rustls::crypto::hash::HashAlgorithm::SHA384)
    };
    let mut tls13_first = true;
    for suite in &p.cipher_suites {
        match suite {
            SupportedCipherSuite::Tls13(s) => {
                if !tls13_first {
                    return Err(format!("suite {:?} comes after a TLS 1.2 suite", s.common.suite));
                }
                if !known(s.common.hash_provider.algorithm()) {
                    return Err(format!("suite {:?} names a hash with no HKDF", s.common.suite));
                }
            }
            SupportedCipherSuite::Tls12(s) => {
                tls13_first = false;
                if !known(s.common.hash_provider.algorithm()) {
                    return Err(format!("suite {:?} names a hash with no PRF", s.common.suite));
                }
                if s.kx != rustls::crypto::KeyExchangeAlgorithm::ECDHE {
                    return Err(format!("suite {:?} has no ephemeral key exchange", s.common.suite));
                }
                if s.sign.is_empty() {
                    return Err(format!("suite {:?} can verify no signature", s.common.suite));
                }
                // A suite whose server signs with a scheme that the provider
                // cannot verify fails after the server chose it.
                let mapped = |scheme: &rustls::SignatureScheme| {
                    p.signature_verification_algorithms.mapping.iter().any(|(m, _)| m == scheme)
                };
                if let Some(scheme) = s.sign.iter().find(|scheme| !mapped(scheme)) {
                    return Err(format!(
                        "suite {:?} signs with {scheme:?}, which no algorithm verifies",
                        s.common.suite
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The provider of this build passes its own check, with RSA or with no
    /// RSA: an RSA suite comes only with the RSA verifier.
    #[test]
    fn the_provider_of_each_build_passes_its_check() {
        assert_eq!(self_check(), Ok(()));
        let p = provider();
        let rsa_suites = p.cipher_suites.iter().filter(|s| format!("{:?}", s.suite()).contains("_RSA_")).count();
        assert_eq!(rsa_suites, if cfg!(feature = "rsa") { 3 } else { 0 });
    }

    /// A TLS 1.2 suite whose server signs with a scheme that no algorithm of
    /// the provider verifies fails the check.
    #[test]
    fn a_suite_that_signs_with_a_scheme_that_nothing_verifies_fails_the_check() {
        static NO_ECDSA: rustls::crypto::WebPkiSupportedAlgorithms = rustls::crypto::WebPkiSupportedAlgorithms {
            all: &[super::super::sign::ED25519],
            mapping: &[(rustls::SignatureScheme::ED25519, &[super::super::sign::ED25519])],
        };
        let mut p = provider();
        p.signature_verification_algorithms = NO_ECDSA;
        let refused = check(&p).expect_err("the ECDSA suites sign with ECDSA");
        assert!(refused.contains("ECDSA_NISTP384_SHA384"), "{refused}");
    }
}
