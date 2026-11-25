//! The dial of a forward origin: TCP, TLS, unix sockets, and TLS over
//! unix. An origin is a service on this host or its network: its name does
//! not go to DNS over HTTPS, and its connection does not go through the
//! proxy.

use std::sync::{Arc, Mutex, OnceLock};

use crate::fail::{IoFail, OrFail, Result, bail};
use pod_core::Class;
use pod_core::{BoxedIo, Stage};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use crate::ingress::{OriginOptions, Service};

/// The dial of the origins of one tunnel: the TLS of the binary, its
/// connector made once, the configuration of the consumer for an origin
/// with `noTLSVerify`, and the origins of that option that a warning named.
///
/// The crate makes no TLS configuration with no check of the certificate
/// (the rule of `pod-tls`): an origin with `noTLSVerify` takes the
/// configuration of [`Origins::with_no_verify`], which the consumer
/// builds. With no such configuration, the dial of that origin fails.
pub struct Origins {
    tls: Arc<pod_tls::Context>,
    verified: OnceLock<TlsConnector>,
    unverified: Option<TlsConnector>,
    seen: Mutex<Vec<String>>,
    report: pod_core::Report,
    words: crate::Words,
    /// True after the note of the first HTTP request to a raw TCP origin.
    noted_http_to_tcp: std::sync::atomic::AtomicBool,
}

impl Origins {
    pub fn new(tls: Arc<pod_tls::Context>) -> Origins {
        Origins {
            tls,
            verified: OnceLock::new(),
            unverified: None,
            seen: Mutex::new(Vec::new()),
            report: pod_core::Report::default(),
            words: crate::Words::default(),
            noted_http_to_tcp: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The TLS configuration of the consumer for an origin with
    /// `noTLSVerify` (cloudflared's `originRequest.noTLSVerify`). The
    /// consumer chooses what it checks; the crate uses it as it is.
    pub fn with_no_verify(mut self, config: Option<Arc<ClientConfig>>) -> Origins {
        self.unverified = config.map(TlsConnector::from);
        self
    }

    /// The connector of an origin with `noTLSVerify`, or why there is none.
    fn no_verify_connector(&self, origin: &str) -> Result<TlsConnector> {
        let Some(connector) = &self.unverified else {
            bail!(
                Stage::Tls,
                Class::Policy,
                "the origin {origin} has noTLSVerify, and the consumer gives no TLS configuration for it \
                 (Host::origin_no_verify): this crate makes no TLS with no check of the certificate"
            );
        };
        self.note_unverified(origin);
        Ok(connector.clone())
    }

    /// The words of the consumer in the answers and the notes.
    pub fn with_words(mut self, words: crate::Words) -> Origins {
        self.words = words;
        self
    }

    pub fn words(&self) -> &crate::Words {
        &self.words
    }

    /// One note for the first HTTP request that comes to a raw TCP origin:
    /// the tunnel answers it with 501.
    pub(crate) fn note_http_to_tcp(&self, service: &Service) {
        if self.noted_http_to_tcp.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let words = &self.words;
        let text = format!(
            "an HTTP request came to the raw TCP origin {}; {} answers 501. {}",
            origin_label(service),
            words.name,
            words.tcp_origin
        );
        self.report.event(pod_core::Event::warn("origin.http_to_tcp", text));
    }

    /// The lines for the user go to `report`.
    pub fn with_report(mut self, report: pod_core::Report) -> Origins {
        self.report = report;
        self
    }

    /// Dial the origin for a forward service.
    pub async fn dial(&self, service: &Service, options: &OriginOptions) -> Result<BoxedIo> {
        dial(self, service, options).await
    }
}

/// Dial the origin for a forward service.
async fn dial(origins: &Origins, service: &Service, options: &OriginOptions) -> Result<BoxedIo> {
    let timeout = options.connect_timeout;
    let connect = async {
        match service {
            Service::Http(url) => {
                let (host, port, tls) = parse_http_url(url)?;
                let unverified = match tls && options.no_tls_verify {
                    true => Some(origins.no_verify_connector(url)?),
                    false => None,
                };
                let tcp = connect_origin(&host, port).await?;
                if tls { origins.tls_wrap(tcp, &host, unverified).await } else { Ok(Box::new(tcp) as BoxedIo) }
            }
            Service::Unix(path) => dial_unix(origins, path, false, options).await,
            Service::UnixTls(path) => dial_unix(origins, path, true, options).await,
            Service::Tcp(addr) => {
                let (host, port) =
                    split_host_port(addr).or_fail(Stage::Origin, Class::Config, format!("{addr} is not HOST:PORT"))?;
                let s = connect_origin(host, port).await?;
                let _ = s.set_nodelay(true);
                Ok(Box::new(s) as BoxedIo)
            }
            other => bail!(Stage::Origin, Class::Config, "{} is not a forward service", other.describe()),
        }
    };
    match timeout {
        Some(limit) => tokio::time::timeout(limit, connect).await.map_err(|_| {
            crate::fail::new(Stage::Origin, Class::Timeout, format!("origin connect timed out after {limit:?}"))
        })?,
        None => connect.await,
    }
}

/// Dial a filesystem socket, with TLS when `tls` is true.
#[cfg(unix)]
async fn dial_unix(origins: &Origins, path: &std::path::Path, tls: bool, options: &OriginOptions) -> Result<BoxedIo> {
    let unverified = match tls && options.no_tls_verify {
        true => Some(origins.no_verify_connector(&format!("unix+tls:{}", path.display()))?),
        false => None,
    };
    let stream =
        UnixStream::connect(path).await.io_fail(Stage::Origin, format!("connecting to unix:{}", path.display()))?;
    if tls { origins.tls_wrap(stream, "localhost", unverified).await } else { Ok(Box::new(stream) as BoxedIo) }
}

#[cfg(not(unix))]
async fn dial_unix(
    _origins: &Origins,
    path: &std::path::Path,
    _tls: bool,
    _options: &OriginOptions,
) -> Result<BoxedIo> {
    bail!(Stage::Origin, Class::Unsupported, "unix sockets are not supported on this platform: {}", path.display())
}

impl Origins {
    /// One warning for each origin whose TLS certificate is not checked
    /// (noTLSVerify): the first dial of it says so; the later dials do not
    /// (a warning for each request fills the log). Gives true for the
    /// first call of `origin`. At most 256 origins are kept.
    pub(crate) fn note_unverified(&self, origin: &str) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.iter().any(|o| o == origin) {
            return false;
        }
        if seen.len() < 256 {
            seen.push(origin.to_string());
        }
        let text = format!("noTLSVerify: the TLS certificate of the origin {origin} is not checked");
        self.report.event(pod_core::Event::warn("origin.no_verify", text));
        true
    }

    /// TLS over `stream`: with the connector of `noTLSVerify` when it is
    /// given, else with the roots of the TLS context.
    async fn tls_wrap<S>(&self, stream: S, server_name: &str, unverified: Option<TlsConnector>) -> Result<BoxedIo>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let connector = unverified.unwrap_or_else(|| self.verified_connector());
        let name = ServerName::try_from(server_name.to_string()).or_fail(
            Stage::Origin,
            Class::Config,
            format!("invalid TLS server name {server_name:?}"),
        )?;
        let tls =
            connector.connect(name, stream).await.io_fail(Stage::Tls, format!("TLS handshake with {server_name}"))?;
        Ok(Box::new(tls))
    }

    fn verified_connector(&self) -> TlsConnector {
        self.verified
            .get_or_init(|| {
                let context = &self.tls;
                let config = context
                    .client_config_with(&[], &[])
                    .map(Arc::new)
                    .or_else(|err| {
                        let text = format!("{err}; the compiled-in roots alone are used");
                        self.report.event(pod_core::Event::warn("origin.roots", text));
                        pod_tls::client_config(
                            &pod_tls::roots_from_compiled_set(),
                            context.provider().clone(),
                            context.groups().clone(),
                        )
                    })
                    .expect("a configuration over the compiled-in roots");
                TlsConnector::from(config)
            })
            .clone()
    }
}

/// The origin path of a service, for the logs.
pub fn origin_label(service: &Service) -> String {
    match service {
        Service::Unix(path) => format!("unix:{}", path.display()),
        Service::UnixTls(path) => format!("unix+tls:{}", path.display()),
        other => other.describe(),
    }
}

/// The addresses of a name of RFC 6761 for this host: `localhost` and
/// `*.localhost`. With no such rule, a static build on a host with no
/// /etc/hosts does not resolve `localhost`.
pub fn local_name(host: &str) -> Option<Vec<std::net::IpAddr>> {
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    (name == "localhost" || name.ends_with(".localhost"))
        .then(|| vec![std::net::IpAddr::from([127, 0, 0, 1]), std::net::IpAddr::from(std::net::Ipv6Addr::LOCALHOST)])
}

/// `HOST:PORT` or `[V6]:PORT`.
pub fn split_host_port(addr: &str) -> Option<(&str, u16)> {
    let (host, port) = addr.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host, port.parse().ok()?))
}

/// Connect to an origin. `localhost` needs no resolver. The error says if
/// the name or the connection failed. An origin name does not go to DNS
/// over HTTPS.
pub async fn connect_origin(host: &str, port: u16) -> Result<TcpStream> {
    let addrs: Vec<std::net::SocketAddr> = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => vec![(ip, port).into()],
        Err(_) => match local_name(host) {
            Some(ips) => ips.into_iter().map(|ip| (ip, port).into()).collect(),
            None => tokio::net::lookup_host((host, port))
                .await
                .io_fail(Stage::Dns, format!("resolving the origin {host}"))?
                .collect(),
        },
    };
    let mut last = None;
    for addr in &addrs {
        match TcpStream::connect(addr).await {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last = Some(crate::fail::io_step(Stage::Origin, format!("connecting to {addr}"), e)),
        }
    }
    Err(last
        .unwrap_or_else(|| crate::fail::new(Stage::Dns, Class::NotFound, format!("the origin {host} has no address"))))
}

/// Parse `http(s)://host[:port]` into `(host, port, tls)`.
pub fn parse_http_url(url: &str) -> Result<(String, u16, bool)> {
    let (scheme, rest) =
        url.split_once("://").or_fail(Stage::Origin, Class::Config, "origin URL is missing a scheme")?;
    let tls = match scheme {
        "http" => false,
        "https" => true,
        other => bail!(Stage::Origin, Class::Config, "unsupported origin scheme {other:?}"),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        bail!(Stage::Origin, Class::Config, "origin URL has no host");
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) =
            rest.split_once(']').or_fail(Stage::Origin, Class::Config, "origin URL has an unclosed IPv6 address")?;
        let port = match rest.strip_prefix(':') {
            Some(port) => port.parse().or_fail(Stage::Origin, Class::Config, "origin URL port is invalid")?,
            None if rest.is_empty() => {
                if tls {
                    443
                } else {
                    80
                }
            }
            None => bail!(Stage::Origin, Class::Config, "origin URL has trailing characters after the IPv6 address"),
        };
        (host.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => {
                let port: u16 = port.parse().or_fail(Stage::Origin, Class::Config, "origin URL port is invalid")?;
                (host.to_string(), port)
            }
            _ => (authority.to_string(), if tls { 443 } else { 80 }),
        }
    };
    if host.is_empty() {
        bail!(Stage::Origin, Class::Config, "origin URL has no host");
    }
    Ok((host, port, tls))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An origin with noTLSVerify and no configuration of the consumer
    /// fails before any connection: the crate makes no TLS with no check.
    #[tokio::test]
    async fn no_tls_verify_needs_the_configuration_of_the_consumer() {
        let origins = Origins::new(crate::testing::tls());
        let options = OriginOptions { no_tls_verify: true, ..OriginOptions::default() };
        let service = Service::Http("https://origin.test.invalid:8443".into());
        let err = origins.dial(&service, &options).await.err().expect("a refused dial");
        assert_eq!((err.stage, err.class), (Stage::Tls, Class::Policy));
        assert!(err.to_string().contains("noTLSVerify"), "{err}");
        let unix = Service::UnixTls("/nonexistent/origin.sock".into());
        let err = origins.dial(&unix, &options).await.err().expect("a refused dial");
        // A system with no Unix socket refuses the origin before its TLS.
        let class = if cfg!(unix) { Class::Policy } else { Class::Unsupported };
        assert_eq!(err.class, class, "{err}");
    }

    /// The warning of an origin with no check of its certificate is an
    /// event for the user, not a line of the crate.
    #[test]
    fn an_unverified_origin_is_an_event() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        let events = move |event: pod_core::Event| kept.lock().unwrap().push(event);
        let report = pod_core::Report { events: Arc::new(events), observer: Arc::new(pod_core::NoObserver) };
        let origins = Origins::new(crate::testing::tls()).with_report(report);
        assert!(origins.note_unverified("https://a.test.invalid:8443"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].level, seen[0].kind), (pod_core::Level::Warn, "origin.no_verify"));
        assert!(seen[0].text.contains("https://a.test.invalid:8443"), "{}", seen[0].text);
    }

    /// The first dial of an origin with noTLSVerify warns; the next
    /// dials of it do not.
    #[test]
    fn an_unverified_origin_is_named_one_time() {
        let origins = Origins::new(crate::testing::tls());
        let note_unverified = |origin: &str| origins.note_unverified(origin);
        assert!(note_unverified("https://a.test.invalid:8443"));
        assert!(!note_unverified("https://a.test.invalid:8443"));
        assert!(note_unverified("https://b.test.invalid:8443"));
    }

    // Localhost needs no resolver (a host with no /etc/hosts).
    #[test]
    fn localhost_is_a_local_name() {
        for name in ["localhost", "LOCALHOST.", "app.localhost"] {
            let ips = local_name(name).unwrap();
            assert_eq!(ips[0], std::net::IpAddr::from([127, 0, 0, 1]), "{name}");
        }
        assert!(local_name("example.com").is_none());
        assert!(local_name("localhost.example.com").is_none());
    }

    #[tokio::test]
    async fn an_origin_on_localhost_connects_and_a_refusal_names_the_address() {
        pod_testkit::need_loopback!();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(connect_origin("localhost", port).await.is_ok());
        drop(listener);
        let err = connect_origin("localhost", port).await.unwrap_err();
        assert!(format!("{err}").contains("connecting to "), "{err}");
        assert!(!format!("{err}").contains("resolving"), "{err}");
        let err = connect_origin("no-such-name.invalid", 80).await.unwrap_err();
        assert!(format!("{err}").contains("resolving the origin"), "{err}");
        assert_eq!(split_host_port("[::1]:8080"), Some(("::1", 8080)));
    }

    /// A refused origin says its cause once in the chain, and keeps the
    /// error of I/O as the cause (PodUtils/podnet#707, ask 10).
    #[tokio::test]
    async fn a_refused_origin_says_its_cause_once() {
        pod_testkit::need_loopback!();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = connect_origin("127.0.0.1", port).await.unwrap_err();
        let all = crate::testing::each_text(&err);
        assert_eq!(all.matches("refused").count(), 1, "{all}");
        assert_eq!(err.chain(), all);
        assert_eq!(err.io_cause().map(|e| e.kind()), Some(std::io::ErrorKind::ConnectionRefused));
        let unix = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
            .io_fail(Stage::Origin, "connecting to unix:/run/o.sock")
            .unwrap_err();
        let all = crate::testing::each_text(&unix);
        assert_eq!(all.matches("refused").count(), 1, "{all}");
        assert_eq!((unix.stage, unix.class), (Stage::Origin, Class::Unavailable));
    }

    #[test]
    fn parses_urls() {
        assert_eq!(parse_http_url("http://127.0.0.1:8080").unwrap(), ("127.0.0.1".into(), 8080, false));
        assert_eq!(parse_http_url("https://example.com").unwrap(), ("example.com".into(), 443, true));
        assert_eq!(parse_http_url("http://example.com/x").unwrap(), ("example.com".into(), 80, false));
        assert_eq!(parse_http_url("https://[::1]:8443").unwrap(), ("::1".into(), 8443, true));
        assert!(parse_http_url("ftp://x").is_err());
        assert!(parse_http_url("http://").is_err());
    }
}
