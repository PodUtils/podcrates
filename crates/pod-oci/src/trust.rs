//! The roots of the requests of the client: the CA bundle of the host first,
//! and the roots compiled into `pod-tls` (`webpki-roots`) when the host has
//! none.
//!
//! A machine whose egress goes through a proxy that signs TLS again has the
//! private CA in its own bundle and nowhere else: a client that trusts only
//! the compiled-in roots fails there with an error that names a
//! certificate, not a proxy. A machine with no bundle is the other case, so
//! the compiled-in roots stay as the fallback.
//!
//! The crate builds no configuration that does not check a certificate. A
//! caller that allows a registry with no check gives its own configuration
//! as a [`pod_tls::Trust`] ([`crate::registry::Client::with_insecure`]).

use std::ffi::OsString;

use rustls_pki_types::CertificateDer;

/// The variables that name a CA bundle, in the order that they are tried.
/// They come before the paths: a proxy that signs TLS again names its root
/// this way. `curl`, `python`, `node` and `cargo` read them too.
pub const BUNDLE_VARS: &[&str] = &["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"];

/// Where a distribution keeps its CA bundle, in the order that they are
/// tried.
pub const BUNDLE_PATHS: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
];

/// The CA bundle of this machine, and whether the machine named it.
pub struct HostBundle {
    pub path: String,
    /// The variable that named the bundle, where one did. A machine that
    /// sets one told each tool which roots to use: a proxy that signs TLS
    /// again, not only the store of the distribution. `None`: the bundle
    /// is at a default path, which names nothing.
    pub announced_by: Option<String>,
    pub bytes: Vec<u8>,
}

/// The certificates of one bundle, read with the PEM reader of `pod-tls`,
/// so that the bundle that this module chooses is one that `pod-tls` reads.
fn load_bundle(path: &str) -> Option<Vec<CertificateDer<'static>>> {
    let data = std::fs::read(path).ok()?;
    pod_tls::bundle::pem_certificates(&data).ok().filter(|certs| !certs.is_empty())
}

/// The CA bundle of the host as the PEM bytes on disk, and where they came
/// from: the first file of [`BUNDLE_VARS`] (read with `var`, the caller's
/// read of its environment), then of [`BUNDLE_PATHS`], that holds at least
/// one certificate. An empty or cut file is no bundle: a copy of it into an
/// image would fail later and further away.
pub fn host_bundle(var: &dyn Fn(&str) -> Option<OsString>) -> Option<HostBundle> {
    let mut candidates: Vec<(String, Option<&str>)> = Vec::new();
    for name in BUNDLE_VARS {
        if let Some(p) = var(name).filter(|p| !p.is_empty()) {
            candidates.push((p.to_string_lossy().to_string(), Some(name)));
        }
    }
    candidates.extend(BUNDLE_PATHS.iter().map(|p| ((*p).to_string(), None)));
    for (path, name) in candidates {
        if load_bundle(&path).is_some()
            && let Ok(bytes) = std::fs::read(&path)
        {
            return Some(HostBundle { path, announced_by: name.map(str::to_string), bytes });
        }
    }
    None
}

/// The roots of `bundle` alone, or the compiled-in roots of `pod-tls` when
/// there is no bundle or no certificate of it parses. `source` says which,
/// and why: "TLS failed" gives no action when the reader cannot see which
/// roots the client read. The count is what the store accepted, not what
/// the file held.
pub fn roots(bundle: Option<&HostBundle>) -> pod_tls::TlsRoots {
    host_roots(bundle).unwrap_or_else(|why| {
        let mut compiled = pod_tls::roots_from_compiled_set();
        compiled.source = format!("webpki-roots, compiled in: {why}");
        compiled
    })
}

/// The roots of the host bundle alone, or why there are none.
fn host_roots(bundle: Option<&HostBundle>) -> Result<pod_tls::TlsRoots, String> {
    let Some(host) = bundle else {
        return Err("this machine has no CA bundle at any known path".to_string());
    };
    let source = match &host.announced_by {
        Some(var) => format!("${var} ({})", host.path),
        None => host.path.clone(),
    };
    let file = pod_tls::CaFile::new(source.clone(), &host.path);
    match pod_tls::roots::load(Some(&file), None, &[]) {
        Ok(roots) => Ok(pod_tls::TlsRoots { count: roots.store.len(), roots: (*roots.store).clone(), source }),
        Err(_) => {
            let offered = pod_tls::bundle::pem_certificates(&host.bytes).map_or(0, |c| c.len());
            Err(format!("{source} held {offered} PEM block(s) and none parsed as a certificate"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_is_built_and_names_where_its_roots_came_from() {
        // The default of the seam, not a double: the roots of this machine,
        // or the compiled-in set.
        let roots = roots(host_bundle(&|_| None).as_ref());
        assert!(roots.count > 0, "no roots from {}", roots.source);
        assert!(!roots.source.is_empty());
        let context = pod_tls::Context::new(
            std::sync::Arc::new(rustls::crypto::ring::default_provider()),
            pod_tls::Settings::new(pod_tls::Words::new("pod-oci")),
        );
        assert!(context.client_config(&roots).is_ok());
        let none = super::roots(None);
        assert!(none.source.starts_with("webpki-roots, compiled in: this machine has no CA bundle"), "{}", none.source);
    }

    #[test]
    fn a_bundle_that_is_not_pem_yields_nothing_rather_than_an_empty_trust_store() {
        let dir = std::env::temp_dir().join(format!("pod-oci-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("not-a-bundle.pem");
        std::fs::write(&p, b"this is not a certificate\n").unwrap();
        assert!(load_bundle(p.to_str().unwrap()).is_none());
        // A variable that names the file does not make it a bundle.
        let named = p.clone().into_os_string();
        let found = host_bundle(&|n| (n == "SSL_CERT_FILE").then(|| named.clone()));
        assert!(found.is_none_or(|b| b.announced_by.is_none()), "a file with no certificate was chosen");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
