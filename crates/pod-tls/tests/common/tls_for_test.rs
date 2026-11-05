// **The providers and the words of the tests.** Included by the tests that
// make a configuration; not a test target of its own.
//
// A test of a configuration runs on ring, and on the pure provider too when
// the feature `pure` is on: the provider that podssh ships is the one that
// its tests proved, and ring is a second stack that this crate did not write.

use std::sync::Arc;

use rustls::crypto::CryptoProvider;

/// Each provider that a test of a configuration runs on, with its name.
pub fn providers() -> Vec<(&'static str, Arc<CryptoProvider>)> {
    #[allow(unused_mut)]
    let mut each = vec![("ring", Arc::new(rustls::crypto::ring::default_provider()))];
    #[cfg(feature = "pure")]
    each.push(("pure", Arc::new(pod_tls::crypto::provider())));
    each
}

/// podssh's words, as podssh gives them to its context.
pub fn podssh_words() -> pod_tls::Words {
    pod_tls::Words {
        app: "podssh".into(),
        replace_roots: "--ca-file FILE or SSL_CERT_FILE".into(),
        add_roots: Some("podssh-ca.pem next to the binary".into()),
        ech_strict: "PODSSH_ECH=strict".into(),
    }
}

/// A context on `provider`, with podssh's words.
pub fn context(provider: Arc<CryptoProvider>) -> pod_tls::Context {
    pod_tls::Context::new(provider, pod_tls::Settings::new(podssh_words()))
}
