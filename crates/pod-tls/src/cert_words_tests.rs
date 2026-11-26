use std::sync::Arc;

use rustls::pki_types::{ServerName, UnixTime, alg_id};
use rustls::{CertificateError, Error, ExtendedKeyPurpose, OtherError};
use webpki::{
    DerTypeId, InvalidNameContext, KeyUsage, RequiredEkuNotFoundContext, UnsupportedSignatureAlgorithmContext,
    UnsupportedSignatureAlgorithmForPublicKeyContext,
};

use super::*;

fn time() -> UnixTime {
    UnixTime::since_unix_epoch(std::time::Duration::from_secs(1_791_440_989))
}

fn name() -> ServerName<'static> {
    ServerName::try_from("other.test").unwrap().to_owned()
}

/// Each variant of rustls's `CertificateError` that can be built, at the
/// version of `Cargo.lock`, but `Other`.
#[allow(deprecated)]
fn each_certificate_error() -> Vec<CertificateError> {
    use CertificateError::*;
    let (time, id) = (time(), vec![1, 2, 3]);
    vec![
        BadEncoding,
        Expired,
        ExpiredContext { time, not_after: time },
        NotValidYet,
        NotValidYetContext { time, not_before: time },
        Revoked,
        UnhandledCriticalExtension,
        UnknownIssuer,
        UnknownRevocationStatus,
        ExpiredRevocationList,
        ExpiredRevocationListContext { time, next_update: time },
        BadSignature,
        UnsupportedSignatureAlgorithm,
        UnsupportedSignatureAlgorithmContext {
            signature_algorithm_id: id.clone(),
            supported_algorithms: vec![alg_id::ED25519],
        },
        UnsupportedSignatureAlgorithmForPublicKeyContext {
            signature_algorithm_id: id.clone(),
            public_key_algorithm_id: id,
        },
        NotValidForName,
        NotValidForNameContext { expected: name(), presented: vec!["DnsName(\"listed.test\")".into()] },
        InvalidPurpose,
        InvalidPurposeContext {
            required: ExtendedKeyPurpose::ServerAuth,
            presented: vec![ExtendedKeyPurpose::ClientAuth],
        },
        InvalidOcspResponse,
        ApplicationVerificationFailure,
    ]
}

/// Each variant of rustls-webpki's `Error` that can be built, at the version
/// of `Cargo.lock`.
#[allow(deprecated)]
fn each_webpki_error() -> Vec<webpki::Error> {
    use webpki::Error::*;
    let time = time();
    let algorithm = || UnsupportedSignatureAlgorithmContext {
        signature_algorithm_id: vec![1],
        supported_algorithms: vec![alg_id::ED25519],
    };
    let for_key = || UnsupportedSignatureAlgorithmForPublicKeyContext {
        signature_algorithm_id: vec![1],
        public_key_algorithm_id: vec![2],
    };
    vec![
        BadDer,
        BadDerTime,
        CaUsedAsEndEntity,
        CertDistrusted,
        CertExpired { time, not_after: time },
        CertNotValidForName(InvalidNameContext { expected: name(), presented: vec!["DnsName(\"a\")".into()] }),
        CertNotValidYet { time, not_before: time },
        CertRevoked,
        CrlExpired { time, next_update: time },
        EmptyEkuExtension,
        EndEntityUsedAsCa,
        ExtensionValueInvalid,
        InvalidCertValidity,
        InvalidCrlNumber,
        InvalidNetworkMaskConstraint,
        InvalidSerialNumber,
        InvalidCrlSignatureForPublicKey,
        InvalidSignatureForPublicKey,
        IssuerNotCrlSigner,
        MalformedDnsIdentifier,
        MalformedExtensions,
        MalformedNameConstraint,
        MaximumNameConstraintComparisonsExceeded,
        MaximumPathBuildCallsExceeded,
        MaximumPathDepthExceeded,
        MaximumSignatureChecksExceeded,
        NameConstraintViolation,
        PathLenConstraintViolated,
        RequiredEkuNotFound,
        RequiredEkuNotFoundContext(RequiredEkuNotFoundContext {
            required: KeyUsage::server_auth(),
            present: vec![vec![1, 3, 6]],
        }),
        SignatureAlgorithmMismatch,
        TrailingData(DerTypeId::Certificate),
        UnknownIssuer,
        UnknownRevocationStatus,
        UnsupportedCertVersion,
        UnsupportedCriticalExtension,
        UnsupportedCrlIssuingDistributionPoint,
        UnsupportedCrlVersion,
        UnsupportedDeltaCrl,
        UnsupportedIndirectCrl,
        UnsupportedNameType,
        UnsupportedRevocationReason,
        UnsupportedRevocationReasonsPartitioning,
        UnsupportedCrlSignatureAlgorithm,
        UnsupportedCrlSignatureAlgorithmContext(algorithm()),
        UnsupportedSignatureAlgorithm,
        UnsupportedSignatureAlgorithmContext(algorithm()),
        UnsupportedCrlSignatureAlgorithmForPublicKey,
        UnsupportedCrlSignatureAlgorithmForPublicKeyContext(for_key()),
        UnsupportedSignatureAlgorithmForPublicKey,
        UnsupportedSignatureAlgorithmForPublicKeyContext(for_key()),
    ]
}

/// Each variant of rustls's `CertificateError`: its name is in the table,
/// and its words, by type and by rustls's text, hold no `Debug` name.
#[test]
fn each_certificate_error_of_rustls_is_said_in_words() {
    for error in each_certificate_error() {
        let debug = format!("{error:?}");
        let name = name_of(&debug);
        assert!(certificate_name_words(name).is_some(), "{name} is not in the table");
        let said = certificate_words(&error);
        assert!(!said.contains(name) && !said.contains("TLS refused"), "{debug} -> {said}");
        let text = Error::InvalidCertificate(error).to_string();
        let plain = plain_certificate(&text);
        assert!(plain.starts_with(PREFIX) && !plain.contains(name), "{text} -> {plain}");
    }
}

/// Each variant of rustls-webpki's `Error`, as rustls gives it in `Other`:
/// the table's words, by type and by rustls's text; each name once in the
/// table.
#[test]
fn each_error_of_webpki_is_said_in_words() {
    let mut names: Vec<&str> = CERTIFICATE_WORDS.iter().map(|(name, _)| *name).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "a name twice in the table");
    for error in each_webpki_error() {
        let debug = format!("{error:?}");
        let words = certificate_name_words(name_of(&debug)).unwrap_or_else(|| panic!("{debug} is not in the table"));
        let other = CertificateError::Other(OtherError(Arc::new(error)));
        assert_eq!(certificate_words(&other), words, "{debug}");
        let text = Error::InvalidCertificate(other).to_string();
        assert_eq!(plain_certificate(&format!("{text}; a hint")), format!("{PREFIX}{words}; a hint"), "{text}");
        assert_eq!(words_in(&text), Some(words), "{text}");
    }
}

/// The refusal of PodUtils/podssh#362: a CA's certificate that a server
/// serves as its own.
#[test]
fn a_ca_used_as_the_server_s_certificate_is_said_in_words() {
    let other = CertificateError::Other(OtherError(Arc::new(webpki::Error::CaUsedAsEndEntity)));
    let text = Error::InvalidCertificate(other.clone()).to_string();
    assert_eq!(text, "invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))");
    let said = plain_certificate(&text);
    assert!(said.starts_with("invalid peer certificate: the certificate of a CA, served as the server's own"));
    assert_eq!(said, format!("{PREFIX}{}", certificate_words(&other)));
}

/// A name with fields loses them; a name that the table does not know keeps
/// its name after words, the last resort; an error of another crate in
/// `Other` gives its own words; text with no refusal is kept.
#[test]
fn fields_go_and_an_unknown_name_is_kept() {
    let with = format!("{PREFIX}Other(OtherError(TrailingData(Certificate))) at the relay");
    assert_eq!(plain_certificate(&with), format!("{PREFIX}{MALFORMED} at the relay"));
    let braces = format!("{PREFIX}CertExpired {{ time: 1, not_after: 2 }}; a hint");
    assert_eq!(plain_certificate(&braces), format!("{PREFIX}{EXPIRED}; a hint"));
    let new = format!("{PREFIX}NewKind");
    assert_eq!(plain_certificate(&new), format!("{PREFIX}a certificate that TLS refused (NewKind)"));
    for kept in ["connection reset", "invalid peer certificate: certificate expired: at 1", ""] {
        assert_eq!(plain_certificate(kept), kept);
    }
    let own = CertificateError::Other(OtherError(Arc::new(std::io::Error::other("the key is not pinned"))));
    assert_eq!(certificate_words(&own), "the key is not pinned");
    #[derive(Debug)]
    struct NewKind;
    impl std::fmt::Display for NewKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("NewKind")
        }
    }
    impl std::error::Error for NewKind {}
    let unknown = CertificateError::Other(OtherError(Arc::new(NewKind)));
    assert_eq!(certificate_words(&unknown), "a certificate that TLS refused (NewKind)");
}
