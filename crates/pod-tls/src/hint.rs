//! The words of a failed handshake, and what to do about it: for an issuer
//! that no root here signs, where a CA goes; for a certificate that is
//! expired or not valid yet, this host's clock and its reading. The names
//! in the words (the program, its flag, its variable, its file) are the
//! consumer's, in [`Words`].

use pod_core::{Class, Failure, Stage};

/// The names of the consumer that the words of a failure give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    /// The name of the program, as its user knows it (`podssh`).
    pub app: String,
    /// Where the user puts a CA that replaces the roots (podssh:
    /// `--ca-file FILE or SSL_CERT_FILE`).
    pub replace_roots: String,
    /// Where the user puts a CA that adds to the roots (podssh:
    /// `podssh-ca.pem next to the binary`); `None` when there is no such
    /// place.
    pub add_roots: Option<String>,
    /// The setting that asks for strict ECH, as the user gives it (podssh:
    /// `PODSSH_ECH=strict`).
    pub ech_strict: String,
}

impl Words {
    /// The words of `app`, with general words for the places of a CA and for
    /// strict ECH.
    pub fn new(app: impl Into<String>) -> Words {
        Words { app: app.into(), replace_roots: "a CA file".into(), add_roots: None, ech_strict: "strict ECH".into() }
    }

    /// What to do about an issuer that no root here signs, as a proxy that
    /// re-signs TLS gives.
    pub fn unknown_issuer_hint(&self) -> String {
        let app = &self.app;
        let mut hint = format!(
            "no root that {app} trusts here signs the certificate: a proxy that re-signs TLS needs its CA, in {}, \
             which replace {app}'s roots",
            self.replace_roots
        );
        if let Some(add) = &self.add_roots {
            hint.push_str(&format!(", or in {add}, which adds to them"));
        }
        hint
    }
}

/// What to do when a TLS handshake failed with `error`: for an issuer that no
/// root here signs, [`Words::unknown_issuer_hint`]; for a certificate that is
/// expired or not valid yet, this host's clock and its reading; none for
/// another failure. The error's type first, its text when a wrapper hides the
/// type.
pub fn handshake_hint(error: &std::io::Error, words: &Words) -> Option<String> {
    use rustls::{CertificateError, Error};
    let inner = error.get_ref().and_then(|inner| inner.downcast_ref::<Error>());
    let text = error.to_string();
    if matches!(inner, Some(Error::InvalidCertificate(CertificateError::UnknownIssuer)))
        || text.contains("UnknownIssuer")
    {
        return Some(words.unknown_issuer_hint());
    }
    let clock = matches!(
        inner,
        Some(Error::InvalidCertificate(
            CertificateError::Expired
                | CertificateError::ExpiredContext { .. }
                | CertificateError::NotValidYet
                | CertificateError::NotValidYetContext { .. }
        ))
    ) || clock_refused(&text);
    clock.then(|| pod_core::clock::hint(std::time::SystemTime::now()))
}

/// Whether the text of a TLS failure says that a certificate's time refused
/// it: rustls writes "certificate expired" and "certificate not valid yet",
/// and their names in a `Debug`.
pub fn clock_refused(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("expired") || lower.contains("notvalidyet") || lower.contains("not valid yet")
}

/// For the text of an error chain that says that a certificate is expired
/// or not valid yet: the clock's words, with this host's reading; else
/// `None`. A wrong clock is a frequent cause on a small host or in a
/// container. The text must name a certificate: an expired token is no
/// fault of the clock. The caller gives the whole chain, with each cause
/// and its `Debug` (podnet: `format!("{err:#} {err:?}")`), so that a
/// wrapper that hides rustls's error does not hide its words.
pub fn clock_hint(chain: &str) -> Option<String> {
    (clock_refused(chain) && chain.to_ascii_lowercase().contains("cert"))
        .then(|| pod_core::clock::hint(std::time::SystemTime::now()))
}

/// The text of a failed handshake, with its hint when it has one.
pub fn handshake_failure(error: &std::io::Error, words: &Words) -> String {
    match handshake_hint(error, words) {
        Some(hint) => format!("{error}; {hint}"),
        None => error.to_string(),
    }
}

/// The failure of a handshake that `error` ended: the words of
/// [`handshake_failure`], the class of the error's kind, and the error as
/// the cause. rustls's own refusal (a certificate, a name, a protocol) is
/// `InvalidData`, so its class is [`Class::Protocol`]; a reset, a close and
/// a time limit keep their kind, which tells how the connection ended.
pub fn handshake_error(error: std::io::Error, words: &Words) -> Failure {
    let text = handshake_failure(&error, words);
    Failure::caused(Stage::Tls, Class::of_io(error.kind()), text, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{CertificateError, Error};

    /// podssh's words.
    fn podssh() -> Words {
        Words {
            app: "podssh".into(),
            replace_roots: "--ca-file FILE or SSL_CERT_FILE".into(),
            add_roots: Some("podssh-ca.pem next to the binary".into()),
            ech_strict: "PODSSH_ECH=strict".into(),
        }
    }

    /// podssh's words give podssh's hint, word for word; with no place that
    /// adds to the roots, the hint names the one that replaces them.
    #[test]
    fn the_words_of_a_consumer_give_its_hint() {
        assert_eq!(
            podssh().unknown_issuer_hint(),
            "no root that podssh trusts here signs the certificate: a proxy that re-signs TLS needs its CA, in \
             --ca-file FILE or SSL_CERT_FILE, which replace podssh's roots, or in podssh-ca.pem next to the binary, \
             which adds to them"
        );
        let plain = Words::new("podnet").unknown_issuer_hint();
        assert!(plain.contains("no root that podnet trusts") && plain.ends_with("which replace podnet's roots"));
    }

    /// The hint comes with rustls's own error, found by its type, and by its
    /// text when a wrapper hides the type; another failure has none.
    #[test]
    fn an_unknown_issuer_gets_the_hint_and_another_failure_none() {
        let words = podssh();
        let typed = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            Error::InvalidCertificate(CertificateError::UnknownIssuer),
        );
        assert_eq!(handshake_hint(&typed, &words), Some(words.unknown_issuer_hint()));
        let said = handshake_failure(&typed, &words);
        assert!(said.contains("UnknownIssuer") && said.contains("--ca-file FILE or SSL_CERT_FILE"), "{said}");
        let text = std::io::Error::other("TLS with the relay failed: invalid peer certificate: UnknownIssuer");
        assert_eq!(handshake_hint(&text, &words), Some(words.unknown_issuer_hint()));
        let name = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            Error::InvalidCertificate(CertificateError::NotValidForName),
        );
        assert_eq!(handshake_hint(&name, &words), None);
        assert_eq!(handshake_failure(&name, &words), name.to_string());
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert_eq!(handshake_hint(&reset, &words), None);
    }

    /// Each of rustls's four errors of a certificate's time gets the clock's
    /// words with its reading, by type and by text; a name mismatch does not.
    #[test]
    fn a_certificate_refused_by_its_time_names_the_clock() {
        let words = podssh();
        let time = rustls_pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(1_791_440_989));
        for e in [
            CertificateError::Expired,
            CertificateError::NotValidYet,
            CertificateError::ExpiredContext { time, not_after: time },
            CertificateError::NotValidYetContext { time, not_before: time },
        ] {
            let typed = std::io::Error::new(std::io::ErrorKind::InvalidData, Error::InvalidCertificate(e));
            let said = handshake_failure(&typed, &words);
            assert!(said.contains("certificates are checked against this host's clock, which reads"), "{said}");
            assert!(said.ends_with(" GMT"), "{said}");
        }
        let text = std::io::Error::other("TLS with the relay failed: invalid peer certificate: certificate expired");
        assert!(handshake_hint(&text, &words).is_some_and(|h| h.contains("this host's clock")));
        let name = std::io::Error::other("invalid peer certificate: NotValidForName");
        assert_eq!(handshake_hint(&name, &words), None);
    }

    /// The hint of an error chain that names an expired certificate gives the
    /// date of this host; another error, and an expired thing that is not a
    /// certificate, get none.
    #[test]
    fn the_clock_hint_names_the_host_date() {
        // podnet's text of `anyhow!("invalid peer certificate: Expired").context("TLS handshake")`.
        let err = "TLS handshake: invalid peer certificate: Expired \
                   TLS handshake\n\nCaused by:\n    invalid peer certificate: Expired";
        let hint = clock_hint(err).unwrap();
        assert!(hint.contains("which reads ") && hint.ends_with(" GMT"), "{hint}");
        assert!(clock_hint("connection refused").is_none());
        assert!(clock_hint("the session token expired").is_none());
        assert!(clock_hint("invalid peer certificate: NotValidYet").is_some());
    }

    /// podnet's `imf_fixdate` of a time is `pod_core::clock::http_date` of its
    /// seconds since the epoch: the same dates.
    #[test]
    fn the_dates_of_imf_fixdate() {
        let at = pod_core::clock::http_date;
        assert_eq!(at(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(at(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(at(951_782_400), "Tue, 29 Feb 2000 00:00:00 GMT");
        assert_eq!(at(2_208_988_800), "Sun, 01 Jan 2040 00:00:00 GMT");
    }

    /// The failure of a handshake keeps the words, the class of the kind, and
    /// the error as its cause, named once in the chain.
    #[test]
    fn a_handshake_error_keeps_its_kind_its_words_and_its_cause() {
        let words = podssh();
        let refused = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            Error::InvalidCertificate(CertificateError::UnknownIssuer),
        );
        let failure = handshake_error(refused, &words);
        assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Protocol));
        assert!(failure.text().contains("UnknownIssuer") && failure.text().contains("--ca-file"), "{failure}");
        assert_eq!(failure.chain(), failure.text());
        let cause = std::error::Error::source(&failure).and_then(|e| e.downcast_ref::<std::io::Error>());
        assert_eq!(cause.map(|e| e.kind()), Some(std::io::ErrorKind::InvalidData));
        let reset = handshake_error(std::io::Error::from(std::io::ErrorKind::ConnectionReset), &words);
        assert_eq!(reset.class, Class::Unavailable);
        let closed = handshake_error(std::io::Error::from(std::io::ErrorKind::UnexpectedEof), &words);
        let kind =
            std::error::Error::source(&closed).and_then(|e| e.downcast_ref::<std::io::Error>()).map(|e| e.kind());
        assert_eq!(kind, Some(std::io::ErrorKind::UnexpectedEof));
    }
}
