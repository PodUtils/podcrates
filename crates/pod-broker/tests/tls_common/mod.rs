//! The stand-ins of the tests of the TLS end (T-100): a far end in memory
//! that ends TLS with a leaf of its own CA and answers each request with a
//! function of it, a broker with the TLS end that trusts that CA, the TLS of
//! the job that trusts the CA of its job, and a recorder of `tracing`.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_broker::ca::JobCa;
use pod_broker::target::Host;
use pod_broker::{Broker, Load, Policy, Secrets, Settings, TlsEnd, Token};
use pod_core::{BoxFuture, BoxedIo, Connected, Dial, Failure, Target};
use pod_proxy::http::{Body, Framing, Response};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;

use crate::common::{Answers, Seen, ask, connect, status_of};

/// A made-up secret value, planted to prove that it never leaks
/// (`.github/secrets-allow.txt`).
pub const SECRET: &str = "made-up-secret-value-T100-a1b2c3d4";
/// The placeholder that the job holds.
pub const PH: &str = "PODCRED_GITHUB_TOKEN_0001";
/// The host of the credential.
pub const HOST: &str = "api.example.com";
/// A host that no credential names, whose far end has a leaf.
pub const OTHER: &str = "other.example.com";
/// A host whose far end has no leaf: its TLS fails.
pub const NO_LEAF: &str = "noleaf.example.com";

/// What the far end answers to one request: its head and its body.
pub type Answer = Arc<dyn Fn(&str, &[u8]) -> Vec<u8> + Send + Sync>;

pub fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(pod_tls::crypto::provider())
}

/// A client configuration that trusts `pem` alone.
pub fn trusting(pem: &str) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in pod_tls::bundle::pem_certificates(pem.as_bytes()).unwrap() {
        roots.add(cert).unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}

/// The far ends: a TLS server with a leaf of `ca` for each name, and the
/// bytes of each request that they read, after their TLS.
pub struct TlsFar {
    pub ca: JobCa,
    pub answer: Answer,
    pub seen: Arc<Mutex<Vec<u8>>>,
    pub dialed: Mutex<Vec<Target>>,
}

impl TlsFar {
    pub fn new(answer: Answer) -> Arc<TlsFar> {
        let ca = JobCa::new("far", Duration::from_secs(3600)).unwrap();
        Arc::new(TlsFar { ca, answer, seen: Arc::default(), dialed: Mutex::default() })
    }

    pub fn seen(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }
}

impl Dial for TlsFar {
    fn label(&self) -> String {
        "tls-far".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        self.dialed.lock().unwrap().push(target.clone());
        let leaves = [HOST, OTHER].map(|name| (name.to_string(), self.ca.leaf(&Host::Name(name.into())).unwrap()));
        let (answer, seen) = (self.answer.clone(), self.seen.clone());
        Box::pin(async move {
            let (ours, theirs) = tokio::io::duplex(256 * 1024);
            let mut config = rustls::ServerConfig::builder_with_provider(provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_cert_resolver(Arc::new(BySni(leaves.to_vec())));
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            let config = Arc::new(config);
            tokio::spawn(async move {
                let Ok(tls) = pod_tls::server::TlsAcceptor::from(config).accept(theirs).await else { return };
                serve(tls, answer, seen).await;
            });
            Ok(Connected::new(Box::new(ours)))
        })
    }
}

/// The leaf of the name that a client asks: the far ends have the leaves
/// of [`HOST`] and [`OTHER`], and no other.
#[derive(Debug)]
struct BySni(Vec<(String, Arc<rustls::sign::CertifiedKey>)>);

impl rustls::server::ResolvesServerCert for BySni {
    fn resolve(&self, hello: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        let name = hello.server_name()?;
        self.0.iter().find(|(n, _)| n == name).map(|(_, pair)| pair.clone())
    }
}

/// Each request of a connection: keep its head and its body, answer.
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(mut io: S, answer: Answer, seen: Arc<Mutex<Vec<u8>>>) {
    let mut rest = Vec::new();
    loop {
        let Ok((request, after)) = pod_proxy::http::read_request(&mut io, std::mem::take(&mut rest)).await else {
            return;
        };
        let framing = if request.has_token("transfer-encoding", "chunked") {
            Framing::Chunked
        } else {
            Framing::Length(request.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0))
        };
        let mut head = format!("{} {} HTTP/1.1\r\n", request.method, request.target);
        for (n, v) in &request.headers {
            let _ = write!(head, "{n}: {v}\r\n");
        }
        let mut body = Vec::new();
        let mut reader = Body::new(&mut io, after, framing, None).unwrap();
        if reader.read_to_end(&mut body).await.is_err() {
            return;
        }
        {
            let mut kept = seen.lock().unwrap();
            kept.extend_from_slice(head.as_bytes());
            kept.extend_from_slice(b"\r\n");
            kept.extend_from_slice(&body);
        }
        if io.write_all(&answer(&head, &body)).await.is_err() {
            return;
        }
        let _ = io.flush().await;
    }
}

/// An answer of 200 with `body`, by its length.
pub fn ok(extra: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 200 OK\r\n{extra}Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

/// The far end echoes the head and the body of each request in its body.
pub fn echo() -> Answer {
    Arc::new(|head: &str, body: &[u8]| {
        let mut all = head.as_bytes().to_vec();
        all.extend_from_slice(body);
        ok("", &all)
    })
}

/// A broker with the TLS end, whose far ends are `far`.
pub fn tls_broker(far: Arc<TlsFar>) -> (Arc<Broker>, Arc<Seen>) {
    tls_broker_with(far, pod_broker::tls_end::Bounds::default())
}

pub fn tls_broker_with(far: Arc<TlsFar>, bounds: pod_broker::tls_end::Bounds) -> (Arc<Broker>, Arc<Seen>) {
    let end = TlsEnd::new(provider(), trusting(far.ca.pem())).with_bounds(bounds);
    let audit = Arc::new(Seen::default());
    let broker = Broker::new(Settings::default(), Answers::public(), far)
        .with_audit(audit.clone())
        .with_tunnel_end(Arc::new(end));
    (Arc::new(broker), audit)
}

/// A policy with one credential for [`HOST`], placed `places` (JSON list),
/// and `allow` as its allow rules (empty: open).
pub fn policy(places: &str, allow: &str) -> String {
    let mode = if allow.is_empty() { r#""mode":"open""# } else { r#""mode":"allow""# };
    let allow = if allow.is_empty() { String::new() } else { format!(r#","allow":{allow}"#) };
    let places = if places.is_empty() { String::new() } else { format!(r#","in":{places}"#) };
    format!(
        r#"{{"schema":1,{mode}{allow},"credentials":[{{"placeholder":"{PH}","secret":"github","hosts":["{HOST}"]{places}}}]}}"#
    )
}

/// Load the job `job1` with `policy` and the planted secret; the PEM of its
/// CA.
pub fn load_secret(broker: &Broker, policy: &str) -> String {
    let policy = Policy::from_json(policy).unwrap();
    let mut load = Load::new("job1", Token::new(crate::common::TOKEN), Duration::from_secs(600), policy);
    load.secrets = Secrets::new().with("github", SECRET);
    broker.load(load).unwrap().ca_pem.expect("a CA for a job with a credential")
}

/// The TLS of the job to `host` through a CONNECT, trusting `ca_pem`, with
/// the name `sni` in its handshake.
pub async fn job_tls(broker: &Arc<Broker>, host: &str, sni: &str, ca_pem: &str) -> BoxedIo {
    let (head, stream) = ask(broker, &connect(&format!("{host}:443"))).await;
    assert_eq!(status_of(&head), 200, "{head}");
    let words = pod_tls::Words::new("test");
    let tls = pod_tls::tls_over(stream, sni, trusting(ca_pem), Duration::from_secs(10), &words).await.unwrap();
    Box::new(tls)
}

/// Send `request` and read its answer, the body decoded.
pub async fn send(io: &mut BoxedIo, request: &[u8]) -> Response {
    io.write_all(request).await.unwrap();
    io.flush().await.unwrap();
    pod_proxy::http::read_response(io, Vec::new(), 8 << 20).await.unwrap()
}

/// The whole answer as text: the status line, each header and the body.
pub fn text(answer: &Response) -> String {
    let mut out = format!("{} {}\n", answer.status, answer.reason);
    for (n, v) in &answer.headers {
        let _ = writeln!(out, "{n}: {v}");
    }
    out.push_str(&answer.text());
    out
}

/// A client configuration that takes any certificate: a job that asks a
/// name that the leaf does not hold still ends its handshake.
pub fn any_certificate() -> Arc<rustls::ClientConfig> {
    let none = JobCa::new("none", Duration::from_secs(60)).unwrap();
    let mut config = (*trusting(none.pem())).clone();
    config.dangerous().set_certificate_verifier(Arc::new(AnyCertificate));
    Arc::new(config)
}

#[derive(Debug)]
struct AnyCertificate;

type Der<'a> = rustls_pki_types::CertificateDer<'a>;

impl rustls::client::danger::ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &Der<'_>,
        _: &[Der<'_>],
        _: &rustls_pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &Der<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &Der<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        provider().signature_verification_algorithms.supported_schemes()
    }
}

/// A recorder of each event of `tracing` on this thread, with its fields.
#[derive(Default, Clone)]
pub struct Logs(Arc<Mutex<String>>);

impl Logs {
    pub fn text(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}

struct Visit<'a>(&'a mut String);

impl tracing::field::Visit for Visit<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

impl tracing::Subscriber for Logs {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut text = self.0.lock().unwrap();
        span.record(&mut Visit(&mut text));
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut Visit(&mut self.0.lock().unwrap()));
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut text = self.0.lock().unwrap();
        event.record(&mut Visit(&mut text));
        text.push('\n');
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
