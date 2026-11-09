//! The trust root for every request podvm makes.
//!
//! ⭐ The host bundle first, `webpki-roots` as the fallback. `Cargo.toml`
//! records why both are pinned: a machine whose egress is intercepted has a
//! private CA in its own bundle and nowhere else, and a client that trusts only
//! the compiled-in roots fails there with an error naming a certificate rather
//! than a proxy. A machine with no bundle at all is the opposite case and is
//! why the compiled-in set is not simply dropped.
//!
//! ⭐ **There IS a "skip verification" path, and it is never taken by default.**
//! [`crate::transport::Policy`] decides, from a `--tls-verify=false`, an
//! `--insecure-registry`, `$PODVM_INSECURE_REGISTRIES` or the config file, and
//! every use of it is announced on stderr.
//! [`TODO/image.md`](../../../TODO/image.md) T-0213.
//!
//! ⚠ **This paragraph used to say the opposite, and it cited a rule that does
//! not exist.** It read: "There is no skip-verification path here, not behind a
//! flag and not behind an environment variable. `docs/security/remote-ops.md`
//! and the environment note in `AGENTS.md` both say never to disable
//! verification". Checked on 2026-09-09: `remote-ops.md` says nothing about TLS
//! at all, and `AGENTS.md`'s note is an instruction to the **agent** about
//! this development container's intercepting proxy, not a rule about what
//! podvm may offer. ⛔ "A switch that exists gets set" is a real argument and
//! is answered rather than dismissed: the switch is off by default, it is
//! per-registry rather than global, and it prints a line naming the registry
//! every time it is used, so a set switch is visible in the transcript of an
//! automated caller rather than inferred from a missing failure.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls_pki_types::CertificateDer;

/// Where a host keeps its CA bundle, in the order they are tried. ⚠ The
/// environment variables come first because that is how an intercepting proxy
/// announces itself; the paths after them are the distributions in the corpus's
/// own `experiments/125-across-distributions.sh` set.
const BUNDLE_ENV: &[&str] = &["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"];
const BUNDLE_PATHS: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
];

/// Which roots a config was built from, for the diagnostic. ⛔ Reported, never
/// assumed: "TLS failed" is unactionable when the reader cannot tell which
/// trust store was consulted.
pub struct Roots {
    pub source: String,
    pub count: usize,
}

/// The certificates of one bundle, read with the PEM reader of `pod-tls`, so
/// that the bundle this module chooses is a bundle that `pod-tls` reads too.
fn load_bundle(path: &str) -> Option<Vec<CertificateDer<'static>>> {
    let data = std::fs::read(path).ok()?;
    pod_tls::bundle::pem_certificates(&data)
        .ok()
        .filter(|certs| !certs.is_empty())
}

/// The host's CA bundle as the PEM bytes on disk, and where they came from.
///
/// ⭐ [`TODO/complete.md`](../../../TODO/complete.md) T-0407 installs this into
/// an image that ships none, so that an `https://` package source can verify.
/// ⛔ It is THIS list rather than a second one: podvm's own transport already
/// had to answer "where does this machine keep its roots", and two lists drift
/// so that the one a reader trusts is the wrong one.
///
/// ⚠ The bytes are returned only where they parse as at least one certificate,
/// so an empty or truncated file reads as "this machine has none" rather than
/// being copied into an image where it would fail later and further away.
pub fn host_bundle_bytes() -> Option<HostBundle> {
    let mut candidates: Vec<(String, Option<&str>)> = Vec::new();
    for var in BUNDLE_ENV {
        if let Some(p) = std::env::var_os(var) {
            candidates.push((p.to_string_lossy().to_string(), Some(var)));
        }
    }
    candidates.extend(BUNDLE_PATHS.iter().map(|p| ((*p).to_string(), None)));
    for (p, var) in candidates {
        if load_bundle(&p).is_some()
            && let Ok(bytes) = std::fs::read(&p)
        {
            return Some(HostBundle {
                path: p,
                announced_by: var.map(str::to_string),
                bytes,
            });
        }
    }
    None
}

/// This machine's CA bundle, and ⭐ **whether the machine ANNOUNCED it**.
pub struct HostBundle {
    pub path: String,
    /// The environment variable that named it, where one did.
    ///
    /// ⭐ **This is the difference between "the host's distro trust store" and
    /// "this machine intercepts TLS and here is the root to use".** `curl`,
    /// `python`, `node` and `cargo` all read `$SSL_CERT_FILE`,
    /// `$CURL_CA_BUNDLE` or `$REQUESTS_CA_BUNDLE`; a machine that sets them has
    /// already told every tool on it which trust store to use, and a container
    /// that ignores them is the anomaly rather than the safe default.
    /// [`TODO/complete.md`](../../../TODO/complete.md) T-0407 turns on this.
    ///
    /// ⚠ `None` means the bundle came from a default path, which is no
    /// announcement at all and is not a reason to touch an image's own trust.
    pub announced_by: Option<String>,
    pub bytes: Vec<u8>,
}

/// A verifier that accepts anything, for a registry the caller named insecure.
///
/// ⛔ Reachable only through [`client_config_unverified`], which
/// [`crate::registry::Client`] calls only for an endpoint
/// [`crate::transport::Policy`] permits. It is not a global mode and there is
/// no environment variable that reaches it without naming a host.
#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The configuration for an endpoint whose certificate is not checked.
///
/// ⚠ The chain is still built and the handshake still happens; what is skipped
/// is deciding whether the chain reaches a trusted root. A caller who sees
/// `Roots.source` here reads exactly that, so a log cannot be mistaken for a
/// verified connection.
pub fn client_config_unverified() -> (Arc<ClientConfig>, Roots) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_no_client_auth();
    (
        Arc::new(config),
        Roots {
            source: "note: NONE: this registry was named insecure, so its \
                     certificate was not verified against any root"
                .to_string(),
            count: 0,
        },
    )
}

/// Build the client configuration and say where its trust came from.
///
/// ⭐ The host bundle is used alone, as `pod-tls` uses a CA file
/// ([`pod_tls::CaFile`]); the compiled-in roots are the fallback for a machine
/// with no bundle, or with a bundle that has no usable root. `pod-tls` makes the
/// configuration: its verifier, the protocol versions and the store of sessions.
pub fn client_config() -> (Arc<ClientConfig>, Roots) {
    let roots = host_roots().unwrap_or_else(|why| {
        let mut compiled = pod_tls::roots_from_compiled_set();
        compiled.source = format!("webpki-roots, compiled in: {why}");
        compiled
    });
    // ⛔ The provider is named rather than taken from the process default.
    // `ClientConfig::builder()` resolves a default that depends on which
    // provider features anything else in the graph enabled, so naming `ring`
    // here is what makes the build's crypto the crypto TODO/deps.md T-0905
    // measured.
    let context = pod_tls::Context::new(
        Arc::new(rustls::crypto::ring::default_provider()),
        pod_tls::Settings::new(pod_tls::Words::new("podvm")),
    );
    let config = context
        .client_config(&roots)
        .expect("a store with at least one root builds its verifier");
    (
        config,
        Roots {
            source: roots.source,
            count: roots.count,
        },
    )
}

/// The roots of the host bundle alone, or why there are none.
fn host_roots() -> Result<pod_tls::TlsRoots, String> {
    let Some(host) = host_bundle_bytes() else {
        return Err("this machine has no CA bundle at any known path".to_string());
    };
    let source = match &host.announced_by {
        Some(var) => format!("${var} ({})", host.path),
        None => host.path.clone(),
    };
    let file = pod_tls::CaFile::new(source.clone(), &host.path);
    match pod_tls::roots::load(Some(&file), None, &[]) {
        // ⚠ The count is what the store ACCEPTED, not what the file held. A
        // bundle whose certificates all fail to parse would otherwise be
        // reported as trust that does not exist.
        Ok(roots) => Ok(pod_tls::TlsRoots {
            count: roots.store.len(),
            roots: (*roots.store).clone(),
            source,
        }),
        Err(_) => {
            let offered = pod_tls::bundle::pem_certificates(&host.bytes).map_or(0, |c| c.len());
            Err(format!(
                "{source} held {offered} PEM block(s) and none parsed as a certificate"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_is_built_and_names_where_its_roots_came_from() {
        // The production default of the seam, not an injected double:
        // docs/conventions/code.md requires the shipping branch be tested.
        let (_cfg, roots) = client_config();
        assert!(roots.count > 0, "no roots from {}", roots.source);
        assert!(!roots.source.is_empty());
    }

    #[test]
    fn a_bundle_that_is_not_pem_yields_nothing_rather_than_an_empty_trust_store() {
        let dir = std::env::temp_dir().join(format!("podvm-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("not-a-bundle.pem");
        std::fs::write(&p, b"this is not a certificate\n").unwrap();
        assert!(load_bundle(p.to_str().unwrap()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
