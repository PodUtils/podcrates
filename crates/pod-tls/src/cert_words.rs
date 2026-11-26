//! A refused certificate in plain words (PodUtils/podssh#362, ask 4). rustls
//! writes most variants of its `CertificateError`, and each error of
//! rustls-webpki in `Other(OtherError(..))`, as their `Debug` names
//! ("invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))"). One
//! table gives each name its words. Only a name that the table does not know
//! (a variant of a later rustls) is said by its name, after words that say
//! that TLS refused a certificate.

use rustls::CertificateError;

/// The words that rustls puts before the refusal of a certificate.
pub(crate) const PREFIX: &str = "invalid peer certificate: ";

/// The words of a certificate that no trusted CA signed.
pub(crate) const UNKNOWN_ISSUER: &str = "a certificate that no trusted CA signed";
const MALFORMED: &str = "a certificate that is not well formed";
const EXPIRED: &str = "an expired certificate";
const NOT_YET: &str = "a certificate that is not valid yet";
const REVOKED: &str = "a revoked certificate";
const OTHER_NAME: &str = "a certificate for another name";
const SIGNATURE: &str = "a certificate whose signature is not correct";
const ALGORITHM: &str = "a certificate signed with an algorithm that this program does not accept";
const PURPOSE: &str = "a certificate that is not for a TLS server";
const EXTENSION: &str = "a certificate with a critical extension that this program does not read";
const REVOCATION_UNKNOWN: &str = "a certificate whose revocation status is not known";
const CRL_EXPIRED: &str = "an expired certificate revocation list";
const CRL: &str = "a certificate revocation list that this program cannot use";
const CHAIN: &str = "a chain of certificates that is too long or too complex to check";
const CONSTRAINT: &str = "a name constraint of a CA that is not well formed";

/// Each `Debug` name of rustls's `CertificateError` and of rustls-webpki's
/// `Error` (the versions of `Cargo.lock`), and its words. A name that both
/// enums have is here once.
#[rustfmt::skip]
pub const CERTIFICATE_WORDS: &[(&str, &str)] = &[
    // rustls's CertificateError.
    ("BadEncoding", MALFORMED),
    ("Expired", EXPIRED),
    ("ExpiredContext", EXPIRED),
    ("NotValidYet", NOT_YET),
    ("NotValidYetContext", NOT_YET),
    ("Revoked", REVOKED),
    ("UnhandledCriticalExtension", EXTENSION),
    ("UnknownIssuer", UNKNOWN_ISSUER),
    ("UnknownRevocationStatus", REVOCATION_UNKNOWN),
    ("ExpiredRevocationList", CRL_EXPIRED),
    ("ExpiredRevocationListContext", CRL_EXPIRED),
    ("BadSignature", SIGNATURE),
    ("UnsupportedSignatureAlgorithm", ALGORITHM),
    ("UnsupportedSignatureAlgorithmContext", ALGORITHM),
    ("UnsupportedSignatureAlgorithmForPublicKeyContext", ALGORITHM),
    ("NotValidForName", OTHER_NAME),
    ("NotValidForNameContext", OTHER_NAME),
    ("InvalidPurpose", PURPOSE),
    ("InvalidPurposeContext", PURPOSE),
    ("InvalidOcspResponse", "a certificate with an OCSP response that is not correct"),
    ("ApplicationVerificationFailure", "a certificate that the program's own check refused"),
    // rustls-webpki's Error, which rustls gives as Other(OtherError(..)).
    ("BadDer", MALFORMED),
    ("BadDerTime", MALFORMED),
    ("CaUsedAsEndEntity", "the certificate of a CA, served as the server's own: the server must serve a certificate that its CA signed"),
    ("CertDistrusted", "a certificate that its trusted root does not vouch for now"),
    ("CertExpired", EXPIRED),
    ("CertNotValidForName", OTHER_NAME),
    ("CertNotValidYet", NOT_YET),
    ("CertRevoked", REVOKED),
    ("CrlExpired", CRL_EXPIRED),
    ("EmptyEkuExtension", PURPOSE),
    ("EndEntityUsedAsCa", "the certificate of a server, used as a CA to sign another"),
    ("ExtensionValueInvalid", MALFORMED),
    ("InvalidCertValidity", MALFORMED),
    ("InvalidCrlNumber", CRL),
    ("InvalidNetworkMaskConstraint", CONSTRAINT),
    ("InvalidSerialNumber", MALFORMED),
    ("InvalidCrlSignatureForPublicKey", CRL),
    ("InvalidSignatureForPublicKey", SIGNATURE),
    ("IssuerNotCrlSigner", CRL),
    ("MalformedDnsIdentifier", MALFORMED),
    ("MalformedExtensions", MALFORMED),
    ("MalformedNameConstraint", CONSTRAINT),
    ("MaximumNameConstraintComparisonsExceeded", CHAIN),
    ("MaximumPathBuildCallsExceeded", CHAIN),
    ("MaximumPathDepthExceeded", CHAIN),
    ("MaximumSignatureChecksExceeded", CHAIN),
    ("NameConstraintViolation", "a certificate for a name that its CA must not sign"),
    ("PathLenConstraintViolated", "a chain of certificates that is longer than its CA allows"),
    ("RequiredEkuNotFound", PURPOSE),
    ("RequiredEkuNotFoundContext", PURPOSE),
    ("SignatureAlgorithmMismatch", SIGNATURE),
    ("TrailingData", MALFORMED),
    ("UnsupportedCertVersion", MALFORMED),
    ("UnsupportedCriticalExtension", EXTENSION),
    ("UnsupportedCrlIssuingDistributionPoint", CRL),
    ("UnsupportedCrlVersion", CRL),
    ("UnsupportedDeltaCrl", CRL),
    ("UnsupportedIndirectCrl", CRL),
    ("UnsupportedNameType", "a certificate for a type of name that this program does not read"),
    ("UnsupportedRevocationReason", CRL),
    ("UnsupportedRevocationReasonsPartitioning", CRL),
    ("UnsupportedCrlSignatureAlgorithm", CRL),
    ("UnsupportedCrlSignatureAlgorithmContext", CRL),
    ("UnsupportedCrlSignatureAlgorithmForPublicKey", CRL),
    ("UnsupportedCrlSignatureAlgorithmForPublicKeyContext", CRL),
    ("UnsupportedSignatureAlgorithmForPublicKey", ALGORITHM),
];

/// The words of the `Debug` name `name` in [`CERTIFICATE_WORDS`].
pub fn certificate_name_words(name: &str) -> Option<&'static str> {
    CERTIFICATE_WORDS.iter().find(|(known, _)| *known == name).map(|(_, words)| *words)
}

/// The words of a name that the table does not know: the last resort.
fn unknown(name: &str) -> String {
    format!("a certificate that TLS refused ({name})")
}

/// The name at the start of a `Debug` form: `Name`, `Name(..)` or
/// `Name { .. }`.
fn name_of(debug: &str) -> &str {
    let len = debug.bytes().take_while(u8::is_ascii_alphanumeric).count();
    &debug[..len]
}

/// The words of a refused certificate: the table's words of its name, or of
/// the name of the error of rustls-webpki in `Other`. rustls's own words of
/// a variant with fields (a name, a time) are kept, as they hold the
/// detail. An error of another crate in `Other` (a pin of `peer`) gives its
/// own words. A name that the table does not know is said by its name.
pub fn certificate_words(error: &CertificateError) -> String {
    if let CertificateError::Other(other) = error {
        let debug = format!("{:?}", other.0);
        let name = name_of(&debug);
        if let Some(words) = certificate_name_words(name) {
            return words.to_string();
        }
        let said = other.0.to_string();
        return match said.is_empty() || said == debug {
            true => unknown(name),
            false => said,
        };
    }
    let said = error.to_string();
    // rustls's Display writes a variant with no words of its own as its Debug.
    if said.starts_with(|c: char| c.is_ascii_lowercase()) {
        return crate::hint::plain_cert_names(&said);
    }
    let debug = format!("{error:?}");
    let name = name_of(&debug);
    certificate_name_words(name).map_or_else(|| unknown(name), str::to_string)
}

/// `text` with the `Debug` name after "invalid peer certificate: " in words:
/// the innermost name of `Other(OtherError(NAME))` or `NAME`, with its
/// fields. Text with no such name is kept.
pub fn plain_certificate(text: &str) -> String {
    let Some(at) = text.find(PREFIX) else { return text.to_string() };
    let start = at + PREFIX.len();
    let rest = &text[start..];
    let inner = rest.strip_prefix("Other(OtherError(").unwrap_or(rest);
    let name = name_of(inner);
    if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
        return text.to_string();
    }
    let wrapped = rest.len() - inner.len();
    let closing = if wrapped > 0 { 2 } else { 0 };
    let after = &inner[name.len()..];
    let end = (start + wrapped + name.len() + debug_end(after) + closing).min(text.len());
    let words = certificate_name_words(name).map_or_else(|| unknown(name), str::to_string);
    format!("{}{words}{}", &text[..start], &text[end..])
}

/// The words of the table that `text` holds after "invalid peer
/// certificate: ", as a `Debug` name or as words already plain.
pub(crate) fn words_in(text: &str) -> Option<&'static str> {
    let rest = &text[text.find(PREFIX)? + PREFIX.len()..];
    let inner = rest.strip_prefix("Other(OtherError(").unwrap_or(rest);
    certificate_name_words(name_of(inner))
        .or_else(|| CERTIFICATE_WORDS.iter().map(|(_, words)| *words).find(|words| rest.starts_with(words)))
}

/// How many bytes of `after` (the text after a name) belong to its `Debug`
/// form: a balanced `(...)` or ` { ... }` right after the name.
fn debug_end(after: &str) -> usize {
    let skip = match after.as_bytes().first() {
        Some(b'(') => 0,
        _ if after.starts_with(" {") => 1,
        _ => return 0,
    };
    let mut depth = 0usize;
    for (i, b) in after.bytes().enumerate().skip(skip) {
        match b {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    after.len()
}

#[cfg(test)]
#[path = "cert_words_tests.rs"]
mod tests;
