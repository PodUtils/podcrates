//! Where the trust anchors come from: a file alone, the default set (the
//! compiled-in roots, an extra bundle, the stores of the system and a
//! system bundle), or the caller's own configuration.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};

use crate::bundle;
use crate::error::Error;

/// The version of the compiled-in Mozilla roots (`webpki-roots`), and the day
/// that crates.io published it: the crate holds no date of its own. A test
/// reads Cargo.lock, so an update of the crate must update both. A binary
/// keeps the roots that it was built with; a doctor can give their age.
pub const ROOTS_VERSION: &str = "1.0.11";
pub const ROOTS_PUBLISHED: &str = "2026-10-23";

/// Where the trust anchors for a server's certificate come from.
///
/// An explicit file (a `--ca-file` or `SSL_CERT_FILE` of the consumer) is
/// used alone and never widened. Otherwise everything that can be found is
/// trusted, so a host with no setup still connects: the extra bundle of the
/// [`crate::Settings`] (podssh keeps one next to its binary), the stores of
/// Windows and macOS ([`crate::stores`]), the first readable system bundle,
/// and the Mozilla root store compiled into the binary (`webpki-roots`).
/// Hostname and chain verification are the same either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// Only the certificates in this PEM file.
    File(PathBuf),
    /// The extra bundle, the system's stores, a system bundle, and the
    /// compiled-in roots.
    Default,
    /// The caller's own configuration, with its provider, roots, verifier and
    /// protocol versions, used as it is. Built only by [`Trust::caller`].
    Caller(CallerConfig),
}

/// A caller's `rustls::ClientConfig` that [`Trust::caller`] accepted.
#[derive(Clone)]
pub struct CallerConfig(Arc<ClientConfig>);

impl CallerConfig {
    pub fn config(&self) -> &Arc<ClientConfig> {
        &self.0
    }
}

impl fmt::Debug for CallerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallerConfig(the caller's rustls::ClientConfig)")
    }
}

/// The same configuration, not an equal one: a `ClientConfig` has no
/// equality, and two of them can differ in a verifier that no field shows.
impl PartialEq for CallerConfig {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CallerConfig {}

impl Trust {
    /// The caller's own TLS configuration, for each connection and each HTTPS
    /// request. Refused when it offers ALPN: a WebSocket upgrade is HTTP/1.1
    /// only, and a server that chose `h2` would not answer it. The
    /// configuration is never changed to fit; its verifier is the caller's,
    /// which this crate cannot check.
    pub fn caller(config: Arc<ClientConfig>) -> Result<Trust, Error> {
        if !config.alpn_protocols.is_empty() {
            let offered: Vec<String> =
                config.alpn_protocols.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect();
            return Err(Error::Config(format!(
                "the caller's TLS configuration offers ALPN ({}); a WebSocket upgrade is \
                 HTTP/1.1 only. Give a configuration with no ALPN: it is used as it is, never changed",
                offered.join(", ")
            )));
        }
        Ok(Trust::Caller(CallerConfig(config)))
    }
}

/// System CA bundle locations, most common first. They are often the same
/// file under different names, so only the first readable one is used. The
/// default of [`crate::Settings::system_bundles`].
pub const SYSTEM_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
    "/usr/local/share/certs/ca-root-nss.crt",
];

#[derive(Debug)]
pub struct TlsRoots {
    pub roots: RootCertStore,
    /// Where they came from, as a doctor prints it. "Verified" is only
    /// meaningful next to the path that was read.
    pub source: String,
    pub count: usize,
}

/// The default trust store: the compiled-in Mozilla roots, plus the
/// `extra` bundle, plus the roots of the system's stores, plus the first
/// readable bundle of `system`. Unreadable or unparsable extras are
/// skipped; the compiled-in set is always there, so this cannot come back
/// empty.
pub(crate) fn default_roots(extra: Option<&Path>, system: &[PathBuf]) -> TlsRoots {
    let mut roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let mut sources = vec![format!(
        "{} compiled-in roots (webpki-roots {ROOTS_VERSION} of {ROOTS_PUBLISHED})",
        webpki_roots::TLS_SERVER_ROOTS.len()
    )];
    if let Some(path) = extra {
        add_extra(&mut roots, &mut sources, path);
    }
    // Where an administrator puts the root of a proxy that re-signs TLS, on
    // systems that keep it outside files.
    for store in crate::stores::read() {
        let (added, _ignored) = roots.add_parsable_certificates(store.roots);
        if added > 0 {
            sources.push(format!("{added} from {}", store.name));
        }
    }
    for candidate in system {
        if add_extra(&mut roots, &mut sources, candidate) {
            break;
        }
    }
    let count = roots.len();
    TlsRoots { roots, source: sources.join(" + "), count }
}

/// Add the parsable certificates of one optional bundle. Returns whether any
/// were added.
fn add_extra(roots: &mut RootCertStore, sources: &mut Vec<String>, path: &Path) -> bool {
    let Ok(certs) = bundle::load_bundle(path) else { return false };
    let (added, _ignored) = roots.add_parsable_certificates(certs);
    if added > 0 {
        sources.push(format!("{added} from {}", path.display()));
    }
    added > 0
}

/// The roots of the PEM file at `path`, alone.
pub fn roots_from_bundle(path: &Path) -> Result<TlsRoots, Error> {
    let certs = bundle::load_bundle(path).map_err(|why| Error::Bundle { path: path.display().to_string(), why })?;
    let offered = certs.len();
    let mut roots = RootCertStore::empty();
    // `add_parsable_certificates` returns `(valid, invalid)` counts. A
    // silently dropped root produces a chain failure that reads as an
    // untrusted issuer rather than as a damaged file, so a non-zero invalid
    // count is an error naming both numbers.
    let (added, invalid) = roots.add_parsable_certificates(certs);
    if invalid > 0 {
        return Err(Error::Bundle {
            path: path.display().to_string(),
            why: format!("{invalid} of {offered} certificate(s) could not be parsed as trust anchors"),
        });
    }
    if added == 0 {
        return Err(Error::Bundle {
            path: path.display().to_string(),
            why: "the bundle produced no usable trust anchors".into(),
        });
    }
    Ok(TlsRoots { roots, source: path.display().to_string(), count: added })
}

/// Only the compiled-in Mozilla roots (`webpki-roots`), with no extras.
pub fn roots_from_compiled_set() -> TlsRoots {
    TlsRoots {
        count: webpki_roots::TLS_SERVER_ROOTS.len(),
        roots: RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() },
        source: format!(
            "the compiled-in webpki-roots set ({} anchors) — NOT a file",
            webpki_roots::TLS_SERVER_ROOTS.len()
        ),
    }
}
