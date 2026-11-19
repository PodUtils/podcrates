//! The CA of one job (T-100): a P-256 key made at the load, a certificate
//! that the job trusts, and a leaf for each host whose tunnel the TLS end
//! reads. The key lives in the memory of the broker only and ends with the
//! job. The job trusts the CA by a bundle file ([`bundle`]) that the host
//! writes, named by each variable of [`pod_tls::BUNDLE_VARS`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pod_acme::key::Key;
use rustls::sign::CertifiedKey;
use rustls_pki_types::CertificateDer;

use crate::target::Host;

/// The CA of one job, and the leaves that it signed.
pub struct JobCa {
    key: Key,
    params: rcgen::CertificateParams,
    pem: String,
    /// The one key of each leaf of the job.
    leaf_key: Key,
    leaves: Mutex<HashMap<String, Arc<CertifiedKey>>>,
    /// The life of each certificate: from a day before the load to a day
    /// after the end of the TTL, so that a clock a day off takes it.
    life: (i64, i64),
}

impl std::fmt::Debug for JobCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The keys never go into a line.
        f.debug_struct("JobCa").field("life", &self.life).finish_non_exhaustive()
    }
}

/// The life of `params`: from the day of `life.0` to the day of `life.1`,
/// at midnight UTC, as rcgen takes a time.
fn set_life(params: &mut rcgen::CertificateParams, life: (i64, i64)) {
    let day = |secs: i64| {
        let (y, m, d) = pod_core::clock::civil_from_days(secs.div_euclid(86_400));
        rcgen::date_time_ymd(y as i32, m as u8, d as u8)
    };
    params.not_before = day(life.0);
    params.not_after = day(life.1);
}

/// A serial number of 16 random bytes: with no crypto feature, rcgen draws
/// none.
fn serial() -> Result<rcgen::SerialNumber, String> {
    use pod_core::entropy::Entropy as _;
    let mut bytes = [0u8; 16];
    pod_core::entropy::OsEntropy.fill(&mut bytes).map_err(|e| format!("no serial number: {e}"))?;
    bytes[0] &= 0x7f;
    Ok(rcgen::SerialNumber::from_slice(&bytes))
}

impl JobCa {
    /// A new CA for the job `job`, for `ttl` from now.
    pub fn new(job: &str, ttl: Duration) -> Result<JobCa, String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let ttl = i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX / 4);
        let life = (now - 86_400, now.saturating_add(ttl).saturating_add(86_400));
        let key = Key::generate()?;
        let mut params = rcgen::CertificateParams::default();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name.push(rcgen::DnType::CommonName, format!("pod-broker CA of the job {job}"));
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign, rcgen::KeyUsagePurpose::DigitalSignature];
        params.serial_number = Some(serial()?);
        set_life(&mut params, life);
        let cert = params.self_signed(&key).map_err(|e| format!("the CA of the job: {e}"))?;
        let pem = pod_acme::certs::pem("CERTIFICATE", cert.der());
        Ok(JobCa { key, params, pem, leaf_key: Key::generate()?, leaves: Mutex::new(HashMap::new()), life })
    }

    /// The certificate of the CA as PEM: the job trusts it.
    pub fn pem(&self) -> &str {
        &self.pem
    }

    /// The leaf of `host` and its key, signed by this CA: made once for
    /// each host of the job.
    pub fn leaf(&self, host: &Host) -> Result<Arc<CertifiedKey>, String> {
        let name = match host {
            Host::Name(name) => name.clone(),
            Host::Ip(ip) => ip.to_string(),
        };
        let mut leaves = self.leaves.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pair) = leaves.get(&name) {
            return Ok(pair.clone());
        }
        let mut params = rcgen::CertificateParams::new(vec![name.clone()]).map_err(|e| format!("{name}: {e}"))?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name.push(rcgen::DnType::CommonName, name.as_str());
        params.serial_number = Some(serial()?);
        set_life(&mut params, self.life);
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        // A strict verifier (OpenSSL with X509_STRICT, as Python 3.13 sets it)
        // needs the key id of the CA in the leaf.
        params.use_authority_key_identifier_extension = true;
        let issuer = rcgen::Issuer::from_params(&self.params, &self.key);
        let cert = params.signed_by(&self.leaf_key, &issuer).map_err(|e| format!("the leaf of {name}: {e}"))?;
        let signing = pod_tls::server::signing_key(self.leaf_key.pem().as_bytes())?;
        let chain = vec![CertificateDer::from(cert.der().to_vec())];
        let pair = pod_tls::server::certified_key(chain, signing).map_err(|e| e.to_string())?;
        leaves.insert(name, pair.clone());
        Ok(pair)
    }
}

/// The bundle that a job trusts: the CA of the job, then the public roots
/// that the host gives (`roots_pem`: a CA file of the system, for example
/// one of [`pod_tls::SYSTEM_BUNDLES`]). A variable of the list replaces the
/// trust of a tool, so the roots keep each other host of the job working.
pub fn bundle(ca_pem: &str, roots_pem: &[u8]) -> Result<String, String> {
    pod_tls::bundle::pem_certificates(ca_pem.as_bytes()).map_err(|e| format!("the CA of the job: {e}"))?;
    let roots = std::str::from_utf8(roots_pem).map_err(|_| "the roots are not PEM text".to_string())?;
    if pod_tls::bundle::pem_certificates(roots_pem).map_err(|e| format!("the roots: {e}"))?.is_empty() {
        return Err("the roots hold no CERTIFICATE block".into());
    }
    let mut out = String::with_capacity(ca_pem.len() + roots.len() + 1);
    out.push_str(ca_pem);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(roots);
    Ok(out)
}

/// Write [`bundle`] to `path` with the mode 0644 (the job's user reads it),
/// in one replace that a power cut does not leave half done.
pub fn write_bundle(path: &Path, ca_pem: &str, roots_pem: &[u8]) -> std::io::Result<()> {
    let text = bundle(ca_pem, roots_pem).map_err(std::io::Error::other)?;
    pod_core::files::replace_private(path, text.as_bytes(), 0o644, false).map(|_| ())
}

/// Each variable that names the bundle of a job, with its path: the
/// environment of the job (`SSL_CERT_FILE`, `GIT_SSL_CAINFO`,
/// `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE` and the others of
/// [`pod_tls::BUNDLE_VARS`]).
pub fn env(path: &Path) -> Vec<(&'static str, PathBuf)> {
    pod_tls::BUNDLE_VARS.iter().map(|var| (*var, path.to_path_buf())).collect()
}
