//! The roots of a context in podnet's order (`Context::roots`): a CA file
//! alone, else the extra bundle, a system bundle and the compiled-in roots;
//! the skipped roots and their counts; the configuration over them, with
//! the extra roots and the ALPN of the caller. The CA of the tests is
//! minted here, in pure Rust.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use pod_tls::roots::load;
use pod_tls::{CaFile, Context, Settings, Words};
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod cert {
    include!("common/cert_for_test.rs");
}

#[allow(dead_code)]
mod tls {
    include!("common/tls_for_test.rs");
}

const NAME: &str = "origin.test";

/// A directory of this test, removed at the end.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let path = std::env::temp_dir().join(format!("pod-tls-roots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory");
        Dir(path)
    }

    /// The file `name` of this directory, with `text`.
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("the file is written");
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A certificate in PEM.
fn pem(der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = b64.as_bytes().chunks(64).map(|c| std::str::from_utf8(c).expect("base64")).collect();
    format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", lines.join("\n"))
}

/// A CA certificate whose subject is `name`, in PEM.
fn ca(name: &str) -> String {
    pem(&cert::encoding::self_signed_inner(name, true).der)
}

/// The CA that signs the server of [`cert::server_for`] for `NAME`.
fn server_ca() -> String {
    ca(&cert::encoding::anchor_name(NAME))
}

/// The DER of an empty SEQUENCE: valid PEM, not a certificate.
const NOT_A_ROOT: &str = "-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n";

fn context(provider: Arc<CryptoProvider>, ca_file: Option<CaFile>, extra: Option<PathBuf>) -> Context {
    let mut settings = Settings::new(Words::new("podnet"));
    settings.system_bundles.clear();
    settings.extra_bundle = extra;
    settings.ca_file = ca_file;
    Context::new(provider, settings)
}

#[test]
fn an_explicit_file_is_used_alone() {
    let dir = Dir::new("alone");
    let path = dir.file("ca.pem", &ca("podnet test CA"));
    let system: Vec<PathBuf> = pod_tls::SYSTEM_BUNDLES.iter().map(PathBuf::from).collect();

    let roots = load(Some(&CaFile::new("SSL_CERT_FILE", path.clone())), None, &system).unwrap();
    assert_eq!(roots.store.len(), 1);
    assert!(roots.sources[0].starts_with("SSL_CERT_FILE"));

    let all = load(None, Some(&path), &[]).unwrap();
    assert_eq!(all.store.len(), 1 + webpki_roots::TLS_SERVER_ROOTS.len(), "{:?}", all.sources);

    let empty = dir.file("empty.pem", "");
    let err = load(Some(&CaFile::new("--ca-file", empty)), None, &[]).unwrap_err();
    assert!(err.to_string().contains("has no certificate"), "{err}");
}

/// A certificate that is not usable as a root is counted, and a file with
/// no usable root names the reason.
#[test]
fn a_skipped_root_is_counted() {
    let dir = Dir::new("skipped");
    let good = ca("podnet test CA");

    let mixed = dir.file("mixed.pem", &format!("{good}{NOT_A_ROOT}"));
    let roots = load(Some(&CaFile::new("--ca-file", mixed.clone())), None, &[]).unwrap();
    assert_eq!(roots.store.len(), 1);
    assert!(roots.sources[0].ends_with("(1 roots, 1 skipped)"), "{:?}", roots.sources);
    let beside = load(None, Some(&mixed), &[]).unwrap();
    assert!(beside.sources[0].ends_with("(1 roots, 1 skipped)"), "{:?}", beside.sources);

    let only_bad = dir.file("bad.pem", NOT_A_ROOT);
    let err = load(Some(&CaFile::new("--ca-file", only_bad)), None, &[]).unwrap_err();
    assert!(err.to_string().contains("the file has 1 certificates, none usable as a root: "), "{err}");
}

/// Each source is named with its counts, in the order of podnet: the extra
/// bundle, the first system bundle with a usable root, the compiled-in
/// roots. A missing extra bundle is passed over; one that is there but is
/// not PEM is an error that names it; a CA file that cannot be read names
/// where its path came from.
#[test]
fn each_source_is_named_in_its_order() {
    let dir = Dir::new("order");
    let extra = dir.file("podnet-ca.pem", &ca("extra CA"));
    let unusable = dir.file("unusable.pem", NOT_A_ROOT);
    let bundle = dir.file("bundle.pem", &format!("{}{}", ca("system CA 1"), ca("system CA 2")));
    let second = dir.file("second.pem", &ca("not read"));
    let system = vec![dir.0.join("absent.pem"), unusable, bundle.clone(), second];

    let roots = load(None, Some(&extra), &system).unwrap();
    assert_eq!(
        roots.sources,
        vec![
            format!("{} (1 roots)", extra.display()),
            format!("{} (2 roots)", bundle.display()),
            format!("webpki-roots {} ({} roots)", pod_tls::ROOTS_VERSION, webpki_roots::TLS_SERVER_ROOTS.len()),
        ]
    );
    assert_eq!(roots.store.len(), 3 + webpki_roots::TLS_SERVER_ROOTS.len());

    let missing = load(None, Some(&dir.0.join("absent.pem")), &[]).unwrap();
    assert_eq!(missing.sources.len(), 1, "{:?}", missing.sources);

    let broken = dir.file("broken.pem", "-----BEGIN CERTIFICATE-----\nMAA=\n");
    let err = load(None, Some(&broken), &[]).unwrap_err();
    assert!(err.to_string().starts_with(&format!("{}: parsing the PEM: ", broken.display())), "{err}");

    let absent = CaFile::new("--ca-file", dir.0.join("absent.pem"));
    let err = load(Some(&absent), None, &[]).unwrap_err();
    assert!(err.to_string().starts_with(&format!("--ca-file {}: reading the file: ", absent.path.display())), "{err}");
    assert_eq!(pod_core::Failure::from(err).class, pod_core::Class::Config);
}

/// A context reads its roots one time: a change of the file after the first
/// read changes nothing, and a failure is kept too.
#[test]
fn a_context_reads_its_roots_one_time() {
    let dir = Dir::new("once");
    let path = dir.file("ca.pem", &ca("podnet test CA"));
    let provider = tls::providers()[0].1.clone();
    let read = context(provider.clone(), Some(CaFile::new("--ca-file", path.clone())), None);
    let first = read.roots().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(Arc::ptr_eq(&first, &read.roots().unwrap()));

    let failed = context(provider, Some(CaFile::new("--ca-file", path.clone())), None);
    let err = failed.roots().unwrap_err();
    dir.file("ca.pem", &ca("podnet test CA"));
    assert_eq!(failed.roots().unwrap_err(), err);
}

/// The configuration offers the ALPN of the caller; extra roots that are
/// not PEM are an error that names them.
#[test]
fn the_configuration_offers_the_alpn_of_the_caller() {
    for (name, provider) in tls::providers() {
        let context = context(provider, None, None);
        let config = context.client_config_with(&[], &[b"http/1.1"]).unwrap();
        assert_eq!(config.alpn_protocols, vec![b"http/1.1".to_vec()], "{name}");
        let none = context.client_config_with(&[], &[]).unwrap();
        assert!(none.alpn_protocols.is_empty(), "{name}");
        let err = context.client_config_with(&[b"-----BEGIN CERTIFICATE-----\n"], &[]).unwrap_err();
        assert!(err.to_string().contains("parsing the extra roots"), "{name}: {err}");
    }
}

/// One handshake with a server that the test CA signs, with `config`; the
/// bytes that the server wrote.
async fn handshake(config: rustls::ClientConfig) -> Result<String, String> {
    let (acceptor, _) = cert::server_for(NAME)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let server = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut tls) = acceptor.accept(stream).await
        {
            let _ = tls.write_all(b"private CA reached").await;
            let _ = tls.shutdown().await;
        }
    });
    let tcp = tokio::net::TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let name = rustls_pki_types::ServerName::try_from(NAME).map_err(|e| e.to_string())?;
    let connect = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp);
    let result = match tokio::time::timeout(pod_testkit::host::scaled(Duration::from_secs(10)), connect).await {
        Err(_) => Err("the handshake did not end".to_string()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(mut tls)) => {
            let mut got = String::new();
            tls.read_to_string(&mut got).await.map_err(|e| e.to_string())?;
            Ok(got)
        }
    };
    server.abort();
    result
}

/// A private CA is trusted only when it is given: as the CA file, which is
/// used alone, or as extra roots. Another CA file does not widen to the
/// default roots.
#[tokio::test]
async fn a_private_ca_is_trusted_only_when_it_is_given() {
    pod_testkit::need_loopback!();
    let dir = Dir::new("private");
    let file = dir.file("ca.pem", &server_ca());
    let other = dir.file("other.pem", &ca("another CA"));
    for (name, provider) in tls::providers() {
        let plain = context(provider.clone(), None, None);
        let refused = handshake(plain.client_config_with(&[], &[]).unwrap()).await.unwrap_err();
        assert!(refused.contains("UnknownIssuer"), "{name}: {refused}");

        let given = context(provider.clone(), Some(CaFile::new("SSL_CERT_FILE", file.clone())), None);
        let reached = handshake(given.client_config_with(&[], &[]).unwrap()).await;
        assert_eq!(reached.as_deref(), Ok("private CA reached"), "{name}");

        let extra = handshake(plain.client_config_with(&[server_ca().as_bytes()], &[]).unwrap()).await;
        assert_eq!(extra.as_deref(), Ok("private CA reached"), "{name}");

        let alone = context(provider.clone(), Some(CaFile::new("SSL_CERT_FILE", other.clone())), None);
        let refused = handshake(alone.client_config_with(&[], &[]).unwrap()).await.unwrap_err();
        assert!(refused.contains("invalid peer certificate"), "{name}: {refused}");
    }
}
