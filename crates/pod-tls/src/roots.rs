//! The roots of a context in podnet's order ([`Context::roots`]), for a
//! consumer that has one trust for the process:
//!
//! 1. The CA file of the user ([`crate::Settings::ca_file`]; podnet:
//!    `--ca-file`, else `SSL_CERT_FILE`). The file is used alone: no other
//!    root is added to it.
//! 2. Else all of these: the extra bundle ([`crate::Settings::extra_bundle`];
//!    podnet: `podnet-ca.pem` beside its binary), the first bundle of
//!    [`crate::Settings::system_bundles`] that has a usable root, and the
//!    Mozilla roots compiled in (webpki-roots [`crate::ROOTS_VERSION`]). The
//!    stores of Windows and macOS are not read here; [`crate::Trust::Default`]
//!    reads them.
//!
//! A certificate that rustls cannot use as a root is skipped with a warning
//! and counted, so that one bad certificate does not make a whole file
//! unusable; a CA file with no usable root is an error that gives the reason.
//! [`Roots::sources`] names each source with its counts, for a doctor or a
//! status. The context reads the roots one time and keeps them, a failure
//! too.
//!
//! A consumer that chooses the trust for each connection gives a
//! [`crate::Trust`] instead (podssh).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::CertificateDer;

use crate::bundle;
use crate::context::Context;
use crate::error::Error;
use crate::trust::ROOTS_VERSION;

/// A CA file that the user gave, and where its path came from (podnet:
/// `--ca-file` or `SSL_CERT_FILE`). The source is the first word of each
/// text about the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaFile {
    pub source: String,
    pub path: PathBuf,
}

impl CaFile {
    pub fn new(source: impl Into<String>, path: impl Into<PathBuf>) -> CaFile {
        CaFile { source: source.into(), path: path.into() }
    }
}

/// The roots, and where they came from.
#[derive(Debug)]
pub struct Roots {
    pub store: Arc<RootCertStore>,
    /// One entry for each source, for example
    /// `SSL_CERT_FILE /etc/corp.pem (2 roots)` or
    /// `/opt/bin/podnet-ca.pem (2 roots, 1 skipped)`.
    pub sources: Vec<String>,
}

impl Roots {
    /// These roots and the roots of the PEM texts `extra` (podnet: the
    /// roots of Cloudflare's edge, for its edge). A certificate of `extra`
    /// that rustls cannot use as a root is passed over; a text that is not
    /// PEM is an error.
    pub fn with_extra(&self, extra: &[&[u8]]) -> Result<RootCertStore, Error> {
        let mut store = (*self.store).clone();
        for pem in extra {
            let certs = bundle::pem_certificates(pem)
                .map_err(|why| Error::Config(format!("parsing the extra roots: {why}")))?;
            for cert in certs {
                let _ = store.add(cert);
            }
        }
        Ok(store)
    }
}

/// Read the roots in the order of the module. `extra` is the extra bundle,
/// read when it is a file; `system` are the system bundles.
pub fn load(ca_file: Option<&CaFile>, extra: Option<&Path>, system: &[PathBuf]) -> Result<Roots, Error> {
    let mut store = RootCertStore::empty();
    let mut sources = Vec::new();
    if let Some(CaFile { source, path }) = ca_file {
        let named = |why: String| Error::Roots(format!("{source} {}: {why}", path.display()));
        let added = add_file(&mut store, path).map_err(named)?;
        if added.roots == 0 {
            return Err(named(match added.skipped.first() {
                None => "the file has no certificate".into(),
                Some(reason) => {
                    format!("the file has {} certificates, none usable as a root: {reason}", added.skipped.len())
                }
            }));
        }
        sources.push(format!("{source} {} ({})", path.display(), added.count()));
        return Ok(Roots { store: Arc::new(store), sources });
    }
    if let Some(path) = extra.filter(|p| p.is_file()) {
        let added = add_file(&mut store, path).map_err(|why| Error::Roots(format!("{}: {why}", path.display())))?;
        if added.roots == 0 {
            tracing::warn!(file = %path.display(), "the extra bundle of roots has no usable root");
        }
        sources.push(format!("{} ({})", path.display(), added.count()));
    }
    for bundle in system {
        if let Ok(added) = add_file(&mut store, bundle)
            && added.roots > 0
        {
            sources.push(format!("{} ({})", bundle.display(), added.count()));
            break;
        }
    }
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    sources.push(format!("webpki-roots {ROOTS_VERSION} ({} roots)", webpki_roots::TLS_SERVER_ROOTS.len()));
    Ok(Roots { store: Arc::new(store), sources })
}

/// The certificates of one file of roots.
struct Added {
    /// The number of roots added.
    roots: usize,
    /// The reason for each certificate that rustls cannot use as a root.
    skipped: Vec<String>,
}

impl Added {
    /// `2 roots`, or `2 roots, 1 skipped`.
    fn count(&self) -> String {
        match self.skipped.len() {
            0 => format!("{} roots", self.roots),
            n => format!("{} roots, {n} skipped", self.roots),
        }
    }
}

/// Add the certificates of the PEM file `path`. A certificate that rustls
/// cannot use as a root is skipped with a warning; a file that cannot be
/// read or parsed is an error, and adds no root.
fn add_file(store: &mut RootCertStore, path: &Path) -> Result<Added, String> {
    let pem = std::fs::read(path).map_err(|e| format!("reading the file: {e}"))?;
    let certs = bundle::pem_certificates(&pem).map_err(|why| format!("parsing the PEM: {why}"))?;
    Ok(add_each(store, certs, path))
}

fn add_each(store: &mut RootCertStore, certs: Vec<CertificateDer<'static>>, path: &Path) -> Added {
    let mut added = Added { roots: 0, skipped: Vec::new() };
    for cert in certs {
        match store.add(cert) {
            Ok(()) => added.roots += 1,
            Err(err) => {
                tracing::warn!(file = %path.display(), error = %err, "a certificate is not usable as a root; it is skipped");
                added.skipped.push(err.to_string());
            }
        }
    }
    added
}

impl Context {
    /// The roots of the settings in podnet's order (see the module), read one
    /// time in this context; a failure is kept too. A consumer calls it at
    /// its start to refuse a CA file that cannot be used.
    pub fn roots(&self) -> Result<Arc<Roots>, Error> {
        self.roots
            .get_or_init(|| {
                let settings = self.settings();
                load(settings.ca_file.as_ref(), settings.extra_bundle.as_deref(), &settings.system_bundles)
                    .map(Arc::new)
            })
            .clone()
    }

    /// A client configuration over [`Context::roots`] and the extra roots
    /// `extra` (PEM texts), which offers the ALPN protocols `alpn`, with the
    /// provider and the groups of this context. Each call makes a new
    /// configuration: the caller keeps it when TLS must resume.
    pub fn client_config_with(&self, extra: &[&[u8]], alpn: &[&[u8]]) -> Result<ClientConfig, Error> {
        let store = self.roots()?.with_extra(extra)?;
        let mut config = crate::context::config_over(store, self.provider().clone(), self.groups().clone())?;
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        Ok(config)
    }
}
