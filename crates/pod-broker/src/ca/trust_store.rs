//! The trust store of a job for the JVM (T-980): the certificates of the
//! bundle as PKCS #12 (RFC 7292), with no password, no MAC and no
//! encryption. The JVM reads no variable of a PEM file: it reads the file
//! of `javax.net.ssl.trustStore`, which [`java_options`] names.

use std::path::Path;

use yasna::models::ObjectIdentifier;
use yasna::{DERWriter, Tag};

/// pkcs7-data (1.2.840.113549.1.7.1).
const DATA: &[u64] = &[1, 2, 840, 113549, 1, 7, 1];
/// pkcs-12 certBag (1.2.840.113549.1.12.10.1.3).
const CERT_BAG: &[u64] = &[1, 2, 840, 113549, 1, 12, 10, 1, 3];
/// x509Certificate of a CertBag (1.2.840.113549.1.9.22.1).
const X509: &[u64] = &[1, 2, 840, 113549, 1, 9, 22, 1];
/// friendlyName (1.2.840.113549.1.9.20): the alias of an entry.
const FRIENDLY_NAME: &[u64] = &[1, 2, 840, 113549, 1, 9, 20];
/// The trusted key usage of the JDK (2.16.840.1.113894.746875.1.1): the
/// JDK loads a certificate with no key as a trusted entry only with it.
const JDK_TRUST: &[u64] = &[2, 16, 840, 1, 113894, 746875, 1, 1];
/// anyExtendedKeyUsage (2.5.29.37.0), the value of [`JDK_TRUST`].
const ANY_USAGE: &[u64] = &[2, 5, 29, 37, 0];

fn oid(arcs: &[u64]) -> ObjectIdentifier {
    ObjectIdentifier::from_slice(arcs)
}

/// A ContentInfo of pkcs7-data that holds `der`.
fn data(w: DERWriter, der: &[u8]) {
    w.write_sequence(|w| {
        w.next().write_oid(&oid(DATA));
        w.next().write_tagged(Tag::context(0), |w| w.write_bytes(der));
    });
}

/// The SafeBag of one trusted certificate with the alias `alias`.
fn cert_bag(w: DERWriter, alias: &str, cert: &[u8]) {
    w.write_sequence(|w| {
        w.next().write_oid(&oid(CERT_BAG));
        w.next().write_tagged(Tag::context(0), |w| {
            w.write_sequence(|w| {
                w.next().write_oid(&oid(X509));
                w.next().write_tagged(Tag::context(0), |w| w.write_bytes(cert));
            })
        });
        w.next().write_set_of(|w| {
            w.next().write_sequence(|w| {
                w.next().write_oid(&oid(FRIENDLY_NAME));
                w.next().write_set_of(|w| w.next().write_bmp_string(alias));
            });
            w.next().write_sequence(|w| {
                w.next().write_oid(&oid(JDK_TRUST));
                w.next().write_set_of(|w| w.next().write_oid(&oid(ANY_USAGE)));
            });
        });
    });
}

/// The trust store of the bundle of `ca_pem` and `roots_pem` (as
/// [`super::bundle`] makes it) in PKCS #12: the CA of the job with the alias
/// `pod-broker-job-ca`, each root with the alias `root-N`.
pub fn trust_store(ca_pem: &str, roots_pem: &[u8]) -> Result<Vec<u8>, String> {
    let bundle = super::bundle(ca_pem, roots_pem)?;
    let certs = pod_tls::bundle::pem_certificates(bundle.as_bytes())?;
    let safe = yasna::construct_der(|w| {
        w.write_sequence_of(|w| {
            for (n, cert) in certs.iter().enumerate() {
                let alias = if n == 0 { "pod-broker-job-ca".to_string() } else { format!("root-{n}") };
                cert_bag(w.next(), &alias, cert.as_ref());
            }
        })
    });
    let safes = yasna::construct_der(|w| w.write_sequence_of(|w| data(w.next(), &safe)));
    Ok(yasna::construct_der(|w| {
        w.write_sequence(|w| {
            w.next().write_u8(3);
            data(w.next(), &safes);
        })
    }))
}

/// Write [`trust_store`] to `path` with the mode 0644, in one replace, as
/// [`super::write_bundle`] writes the bundle.
pub fn write_trust_store(path: &Path, ca_pem: &str, roots_pem: &[u8]) -> std::io::Result<()> {
    let der = trust_store(ca_pem, roots_pem).map_err(std::io::Error::other)?;
    pod_core::files::replace_private(path, &der, 0o644, false).map(|_| ())
}

/// The value of `JAVA_TOOL_OPTIONS` for a job: the trust store at `store`
/// as the trust of each JVM, then the value of the job (`job`) as it is.
/// The JVM takes the last value of a property, so a trust store that the
/// job names itself keeps its priority. A path with a space, a quote or a
/// character that is not printable ASCII is an error: the JVM splits the
/// variable at each space.
pub fn java_options(store: &Path, job: Option<&str>) -> Result<String, String> {
    let path =
        store.to_str().filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\''));
    let Some(path) = path else {
        return Err(format!("{} cannot be a value of JAVA_TOOL_OPTIONS", store.display()));
    };
    let mut out = format!("-Djavax.net.ssl.trustStore={path} -Djavax.net.ssl.trustStoreType=PKCS12");
    if let Some(job) = job.map(str::trim).filter(|j| !j.is_empty()) {
        out.push(' ');
        out.push_str(job);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CA: &str = "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n";
    const ROOT: &str = "-----BEGIN CERTIFICATE-----\nBAUG\n-----END CERTIFICATE-----\n";

    #[test]
    fn the_trust_store_holds_each_certificate_with_the_trust_of_the_jdk() {
        let der = trust_store(CA, ROOT.as_bytes()).unwrap();
        // The version 3, then the bytes of each certificate in its bag.
        assert_eq!(&der[..2], &[0x30, 0x82][..], "{der:02x?}");
        let has = |needle: &[u8]| der.windows(needle.len()).any(|w| w == needle);
        assert!(has(&[0x04, 0x03, 1, 2, 3]) && has(&[0x04, 0x03, 4, 5, 6]), "{der:02x?}");
        // The attribute of the trust of the JDK, as keytool writes it.
        let jdk_trust = [0x06, 0x0c, 0x60, 0x86, 0x48, 0x01, 0x86, 0xf9, 0x66, 0xad, 0xca, 0x7b, 0x01, 0x01];
        let any_usage = [0x31, 0x06, 0x06, 0x04, 0x55, 0x1d, 0x25, 0x00];
        assert!(has(&[&jdk_trust[..], &any_usage].concat()), "{der:02x?}");
        assert!(has(&yasna::construct_der(|w| w.write_bmp_string("root-1"))), "{der:02x?}");
        assert!(trust_store(CA, b"").is_err(), "no roots");
    }

    #[test]
    fn the_options_name_the_store_before_the_value_of_the_job() {
        let store = Path::new("/run/job/ca.p12");
        let want = "-Djavax.net.ssl.trustStore=/run/job/ca.p12 -Djavax.net.ssl.trustStoreType=PKCS12";
        assert_eq!(java_options(store, None).unwrap(), want);
        assert_eq!(java_options(store, Some("  ")).unwrap(), want);
        assert_eq!(java_options(store, Some("-Xmx1g")).unwrap(), format!("{want} -Xmx1g"));
        assert!(java_options(Path::new("/run/a job/ca.p12"), None).is_err(), "a space");
        assert!(java_options(Path::new("/run/'job/ca.p12"), None).is_err(), "a quote");
    }
}
