//! RFC 8484's wire form against CoreDNS, a resolver that this crate did not
//! write (call C151, T-350): CoreDNS serves a zone of the test over DNS
//! over HTTPS on 127.0.0.1, with a certificate for the address that the
//! test makes. The client asks over HTTP/1.1 and over HTTP/2, by GET: the
//! addresses of a name, AAAA when A gave none, the SRV records, NXDOMAIN at
//! the first answer, and a certificate of another root refused.
//!
//! CoreDNS is the program `POD_DNS_TEST_COREDNS_BIN`, else `coredns` on the
//! path. A host with neither skips each test with the reason, unless
//! `POD_DNS_TEST_COREDNS=required`, which `scripts/ci/coredns-tests.sh`
//! sets with the pinned release.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use pod_dns::{Doh, Form, NXDOMAIN, Settings, Srv, Way};
use pod_proxy::{Dialer, ProxyChoice};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::time::Instant;

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code, unused_imports)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

/// The address of CoreDNS, which its certificate names.
const AT: &str = "127.0.0.1";
/// The bound of each question, and of the start of CoreDNS.
const LIMIT: Duration = Duration::from_secs(20);

/// The zone of the tests: two addresses of one name, a name with an IPv6
/// address alone, and two SRV records.
const ZONE: &str = "$ORIGIN test.
@ 3600 IN SOA ns.test. admin.test. 1 7200 3600 1209600 3600
@ 3600 IN NS ns.test.
ns 3600 IN A 192.0.2.53
host 3600 IN A 192.0.2.7
host 3600 IN A 192.0.2.8
six 3600 IN AAAA 2001:db8::6
_svc._tcp 3600 IN SRV 10 5 7844 region1.test.
_svc._tcp 3600 IN SRV 20 5 7844 region2.test.
";

/// `POD_DNS_TEST_COREDNS=required`: a skip is a failure.
fn required() -> bool {
    std::env::var("POD_DNS_TEST_COREDNS").is_ok_and(|v| v == "required")
}

/// The program of CoreDNS that this host has.
fn program() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("POD_DNS_TEST_COREDNS_BIN").filter(|n| !n.is_empty()) {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("coredns")).find(|p| p.is_file())
}

/// A port of the loopback that is free now.
fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}

/// PEM of `der` with the label `label`.
fn pem(label: &str, der: &[u8]) -> String {
    let text = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in text.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).unwrap());
        out.push('\n');
    }
    out + &format!("-----END {label}-----\n")
}

/// The key of `leaf` as RFC 5915's ECPrivateKey, which CoreDNS reads.
fn key_pem(leaf: &cert::encoding::TestCert) -> String {
    use cert::encoding::{oid, seq, tlv};
    let signing = p256::ecdsa::SigningKey::from_slice(&leaf.key_der).expect("a P-256 scalar");
    let point = signing.verifying_key().to_sec1_point(false);
    let mut body = vec![0x02, 0x01, 0x01, 0x04, 0x20];
    body.extend_from_slice(&leaf.key_der);
    body.extend_from_slice(&tlv(0xa0, &oid(&[1, 2, 840, 10045, 3, 1, 7])));
    body.extend_from_slice(&tlv(0xa1, &tlv(0x03, &[&[0x00][..], point.as_bytes()].concat())));
    pem("EC PRIVATE KEY", &seq(&[body]))
}

/// CoreDNS on 127.0.0.1, serving [`ZONE`] over DNS over HTTPS.
struct CoreDns {
    port: u16,
    /// The root that its certificate chains to.
    roots: rustls::RootCertStore,
    child: Child,
    dir: PathBuf,
}

impl CoreDns {
    /// CoreDNS started, or `None` when this host has none.
    fn start(tag: &str) -> Option<CoreDns> {
        let Some(program) = program() else {
            let why = "no CoreDNS here (set POD_DNS_TEST_COREDNS_BIN, or put coredns on the path)";
            assert!(!required(), "POD_DNS_TEST_COREDNS=required: {why}");
            eprintln!("did not run: {why}");
            return None;
        };
        let dir = std::env::temp_dir().join(format!("pod-dns-coredns-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let anchor = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(AT), true);
        let leaf = cert::encoding::self_signed_inner(AT, false);
        let mut roots = rustls::RootCertStore::empty();
        roots.add(rustls_pki_types::CertificateDer::from(anchor.der)).expect("the root of the test");
        std::fs::write(dir.join("cert.pem"), pem("CERTIFICATE", &leaf.der)).unwrap();
        std::fs::write(dir.join("key.pem"), key_pem(&leaf)).unwrap();
        std::fs::write(dir.join("test.zone"), ZONE).unwrap();
        let port = free_port();
        let corefile = format!(
            "https://test.:{port} {{\n  bind {AT}\n  tls {cert} {key}\n  file {zone}\n  errors\n  log\n}}\n",
            cert = dir.join("cert.pem").display(),
            key = dir.join("key.pem").display(),
            zone = dir.join("test.zone").display(),
        );
        std::fs::write(dir.join("Corefile"), corefile).unwrap();
        let child = Command::new(&program)
            .arg("-conf")
            .arg(dir.join("Corefile"))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(dir.join("coredns.log")).unwrap())
            .stderr(std::fs::File::create(dir.join("coredns.err")).unwrap())
            .spawn()
            .expect("CoreDNS starts");
        let mut dns = CoreDns { port, roots, child, dir };
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let deadline = std::time::Instant::now() + pod_testkit::host::scaled(LIMIT);
        while TcpStream::connect_timeout(&at, Duration::from_millis(200)).is_err() {
            let ended = dns.child.try_wait().ok().flatten();
            assert!(ended.is_none() && std::time::Instant::now() < deadline, "CoreDNS did not listen: {}", dns.said());
            std::thread::sleep(Duration::from_millis(100));
        }
        Some(dns)
    }

    /// What CoreDNS wrote, for a message.
    fn said(&self) -> String {
        let read = |name: &str| std::fs::read_to_string(self.dir.join(name)).unwrap_or_default();
        format!("{}{}", read("coredns.log"), read("coredns.err"))
    }

    /// The resolver of CoreDNS, in the wire form, over HTTP/2 alone or
    /// over HTTP/1.1.
    fn resolver(&self, http2: bool) -> Doh {
        Doh::new(AT.parse().unwrap(), AT).operator("coredns").port(self.port).wire("/dns-query").http2(http2)
    }
}

impl Drop for CoreDns {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The TLS context of the tests, on ring.
fn context() -> Context {
    Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podssh")))
}

/// The settings with `resolvers`, the trust of `roots` alone and no cache.
fn settings(context: &Context, roots: rustls::RootCertStore, resolvers: Vec<Doh>) -> Settings {
    let roots = TlsRoots { count: roots.len(), roots, source: "the test's own root".into() };
    let trust = Trust::caller(context.client_config(&roots).unwrap()).unwrap();
    Settings { resolvers, trust, memory: false, ..Settings::default() }
}

/// A direct dial over the system resolver.
fn dialer() -> Dialer {
    Dialer::new(pod_proxy::Settings::new("podssh", ProxyChoice::Direct), Arc::new(pod_proxy::resolve::System::new()))
}

fn deadline() -> Instant {
    Instant::now() + pod_testkit::host::scaled(LIMIT)
}

fn ips(list: &[&str]) -> Vec<IpAddr> {
    list.iter().map(|s| s.parse().unwrap()).collect()
}

/// Over HTTP/1.1 and over HTTP/2: the two addresses of a name in the wire
/// form, the IPv6 address of a name with no IPv4 address, and the SRV
/// records.
#[tokio::test]
async fn coredns_answers_the_wire_form_over_http1_and_http2() {
    pod_testkit::need_loopback!();
    let Some(dns) = CoreDns::start("forms") else { return };
    let (context, dial) = (context(), dialer());
    for (http2, protocol) in [(false, "HTTP/1.1"), (true, "HTTP/2")] {
        let resolver = dns.resolver(http2);
        let settings = settings(&context, dns.roots.clone(), vec![resolver.clone()]);
        let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
        let asked = way.ask(&resolver, "host.test", deadline()).await;
        let (mut got, form) = asked.unwrap_or_else(|e| panic!("{protocol}: {e}: {}", dns.said()));
        got.sort();
        assert_eq!((got, form), (ips(&["192.0.2.7", "192.0.2.8"]), Form::Wire), "{protocol}");
        let six = way.lookup("six.test", deadline()).await;
        assert_eq!(six, Ok(ips(&["2001:db8::6"])), "{protocol}");
        let mut srv = way.srv("_svc._tcp.test", deadline()).await.unwrap_or_else(|e| panic!("{protocol}: {e}"));
        srv.sort_by_key(|r| r.priority);
        let region = |priority: u16, n: u8| Srv { priority, weight: 5, port: 7844, target: format!("region{n}.test") };
        assert_eq!(srv, [region(10, 1), region(20, 2)], "{protocol}");
        println!("CoreDNS: the wire form answered over {protocol}");
    }
}

/// NXDOMAIN of the zone ends the lookup at the first answer: the second
/// resolver gets no connection.
#[tokio::test]
async fn coredns_nxdomain_ends_the_lookup_at_the_first_answer() {
    pod_testkit::need_loopback!();
    let Some(dns) = CoreDns::start("nxdomain") else { return };
    let second = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    second.set_nonblocking(true).unwrap();
    let next = dns.resolver(false).operator("second").port(second.local_addr().unwrap().port());
    for http2 in [false, true] {
        let context = context();
        let settings = settings(&context, dns.roots.clone(), vec![dns.resolver(http2), next.clone()]);
        let dial = dialer();
        let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
        let missing = way.lookup("no-such-name.test", deadline()).await;
        let why = missing.expect_err("the zone has no such name");
        assert!(why.ends_with(NXDOMAIN), "{why}");
        let connected = second.accept().map(|_| ());
        assert!(connected.is_err_and(|e| e.kind() == std::io::ErrorKind::WouldBlock), "the second was asked");
    }
    println!("CoreDNS: NXDOMAIN ended the lookup");
}

/// A certificate that chains to another root is refused, over each
/// protocol.
#[tokio::test]
async fn coredns_with_a_certificate_of_another_root_is_refused() {
    pod_testkit::need_loopback!();
    let Some(dns) = CoreDns::start("refused") else { return };
    let (_, other) = cert::server_for("other.test").expect("another root");
    let (context, dial) = (context(), dialer());
    for http2 in [false, true] {
        let settings = settings(&context, other.clone(), vec![dns.resolver(http2)]);
        let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
        let refused = way.lookup("host.test", deadline()).await;
        let why = refused.expect_err("a root that the test does not trust");
        assert!(why.contains("UnknownIssuer"), "{why}");
    }
    println!("CoreDNS: a certificate of another root was refused");
}
