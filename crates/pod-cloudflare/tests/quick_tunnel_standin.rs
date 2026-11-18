//! The quick tunnel of the crate against a stand-in of the service and of
//! the edge: no call to Cloudflare. `src/standin.rs` has the stand-in. The
//! tests of podnet's command line on the same stand-in (its JSON lines, its
//! signals, its detached instances) stay in podnet.

use std::time::{Duration, Instant};

use pod_cloudflare::config::Protocol;
use pod_cloudflare::ingress::Service;
use pod_cloudflare::standin::{self, Answer, Reply, Standin, said};
use pod_core::Class;

fn hostname_of(url: Option<&str>) -> String {
    let url = url.expect("a url");
    url.strip_prefix("https://").expect("an https url").into()
}

#[test]
fn quick_tunnel_standin_serves_a_visitor_over_http2() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (port, seen) = standin::origin("origin says hello\n");
    let url = format!("http://127.0.0.1:{port}");
    let (host, _) = standin.host(true, None);
    let handle = standin.run(host, standin.config(Service::Http(url))).expect("the tunnel starts");
    let name = hostname_of(handle.url());
    assert!(name.ends_with(".trycloudflare.com"), "{name}");
    assert_eq!(handle.leg(), "http2");
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));

    let issued = standin.api.issued.lock().unwrap().clone();
    assert_eq!(issued.len(), 1);
    assert_eq!(handle.tunnel_id(), issued[0].id);
    assert_eq!(name, issued[0].hostname);

    let (status, headers, body) = standin.visit(&name, "/some/path?q=1", &[("X-Visitor", "yes")]);
    assert_eq!((status, body.as_str()), (200, "origin says hello\n"));
    assert!(headers.iter().any(|(n, v)| n.eq_ignore_ascii_case("x-origin") && v == "standin"), "{headers:?}");
    let head = seen.lock().unwrap().first().cloned().expect("the origin got a request");
    assert!(head.starts_with("GET /some/path?q=1 "), "{head}");
    assert!(head.to_lowercase().contains("x-visitor: yes"), "{head}");

    // The edge got the tunnel secret on the wire, verbatim. The HA legs
    // after leg 0 register in the background: a slow host can show more
    // than one registration here, each with its own index.
    let registrations = standin.edge.registrations.lock().unwrap().clone();
    let ha = usize::from(standin.config(Service::HelloWorld).ha_connections);
    assert!((1..=ha).contains(&registrations.len()), "{registrations:?}");
    let mut indexes: Vec<u8> = registrations.iter().map(|r| r.conn_index).collect();
    indexes.sort_unstable();
    indexes.dedup();
    assert_eq!(indexes.len(), registrations.len(), "{registrations:?}");
    assert!(indexes.contains(&0), "{registrations:?}");
    for registration in &registrations {
        assert!(registration.accepted);
        assert_eq!(registration.account_tag, issued[0].account_tag);
        assert_eq!(registration.secret, issued[0].secret);
        assert_eq!(registration.tunnel_id, issued[0].id);
    }

    // A shutdown unregisters the connection.
    standin.block_on(handle.shutdown());
    assert!(standin.block_on(standin.edge.wait_closed(&name, Duration::from_secs(5))), "a connection stays");
    assert!(standin.edge.unregistrations.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    let (status, _, body) = standin.visit(&name, "/", &[]);
    assert_eq!(status, 530, "{body}");
}

#[test]
fn quick_tunnel_standin_retries_after_429() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    standin.api.script([Answer::TooMany { retry_after: Some(1) }]);
    let started = Instant::now();
    let (host, events) = standin.host(true, None);
    let handle = standin.run(host, standin.config(Service::HelloWorld)).expect("the tunnel starts");
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(standin.api.hits(), 2, "the first request got 429");
    assert!(said(&events, "provisioning failed"), "{:?}", events.lock().unwrap());
    let name = hostname_of(handle.url());
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));
    standin.block_on(handle.shutdown());
}

/// A wait longer than the budget is a temporary refusal: the class TempFail
/// (exit 75), with the status and the wait in the text.
#[test]
fn quick_tunnel_standin_exits_75_when_retry_after_exceeds_the_budget() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    standin.api.always(Answer::TooMany { retry_after: Some(3600) });
    let (host, _) = standin.host(true, None);
    let err = standin.run(host, standin.config(Service::HelloWorld)).err().expect("the start gives up at once");
    assert_eq!(err.class, Class::TempFail, "{err}");
    assert_eq!(err.class.exit_code(), 75);
    let text = err.to_string();
    assert!(text.contains("429") && text.contains("3600"), "{text}");
    assert_eq!(standin.api.hits(), 1, "no second request");
    assert!(standin.edge.registrations.lock().unwrap().is_empty());
}

#[test]
fn quick_tunnel_standin_does_not_retry_a_403() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    standin.api.always(Answer::Status(403));
    let (host, _) = standin.host(true, None);
    let err = standin.run(host, standin.config(Service::HelloWorld)).err().expect("the start stops");
    assert_eq!(err.class.exit_code(), 69, "{err}");
    assert_eq!(standin.api.hits(), 1);
}

#[test]
fn quick_tunnel_standin_checks_the_auth_user() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let pass = standin.dir.path().join("pass");
    std::fs::write(&pass, "standin-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&pass, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut config = standin.config(Service::HelloWorld);
    config.ingress.auth = Some(pod_cloudflare::ingress::credentials("live", &pass).expect("the credential"));
    let (host, _) = standin.host(true, None);
    let handle = standin.run(host, config).expect("the tunnel starts");
    let name = hostname_of(handle.url());
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));
    let basic = |user: &str, password: &str| {
        let pair = format!("{user}:{password}");
        // HTTP Basic uses the padded form of base64.
        let mut text = pod_cloudflare::cloudflare::http2::base64_encode(pair.as_bytes());
        while !text.len().is_multiple_of(4) {
            text.push('=');
        }
        format!("Basic {text}")
    };
    let none = standin.visit(&name, "/", &[]);
    assert_eq!(none.0, 401, "{none:?}");
    let wrong = standin.visit(&name, "/", &[("Authorization", &basic("live", "wrong!"))]);
    assert_eq!(wrong.0, 401, "{wrong:?}");
    let right = standin.visit(&name, "/", &[("Authorization", &basic("live", "standin-secret"))]);
    assert_eq!(right.0, 200, "{right:?}");
    assert!(right.2.contains("Hello, world"), "{}", right.2);
    standin.block_on(handle.shutdown());
}

#[test]
fn quick_tunnel_standin_goes_through_a_connect_proxy() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (proxy, log) = standin.proxy();
    let proxy = pod_proxy::Proxy::parse(&format!("http://{proxy}")).expect("a proxy");
    let dial = standin::proxied(pod_proxy::ProxyChoice::Via(proxy));
    let (host, events) = standin.host(true, Some(dial));
    let config = pod_cloudflare::config::Config { protocol: Protocol::Auto, ..standin.config(Service::HelloWorld) };
    let handle = standin.run(host, config).expect("the tunnel starts");
    let name = hostname_of(handle.url());
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));
    // The edge leg went through the proxy, and no QUIC leg was tried.
    let seen = log.lock().unwrap().clone();
    assert!(seen.contains(&standin.edge_addr.to_string()), "{seen:?}");
    assert!(said(&events, "no proxy carries UDP"), "{:?}", events.lock().unwrap());
    standin.block_on(handle.shutdown());
}

#[test]
fn quick_tunnel_standin_exits_when_the_edge_refuses_the_registration() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    standin.edge.answer(Reply::Refuse("the tunnel is deleted".into()));
    let (host, _) = standin.host(true, None);
    let err = standin.run(host, standin.config(Service::HelloWorld)).err().expect("the start gives up");
    assert!(err.to_string().contains("the tunnel is deleted"), "{err}");
    let registrations = standin.edge.registrations.lock().unwrap().clone();
    assert!(!registrations.is_empty() && registrations.iter().all(|r| !r.accepted));
}

#[test]
fn quick_tunnel_standin_front_answers_530_for_an_unknown_host() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (status, _, body) = standin.visit("nobody.trycloudflare.com", "/", &[]);
    assert_eq!(status, 530);
    assert!(body.contains("1033"), "{body}");
}

/// With UDP on and the protocol auto, the QUIC leg gets no answer (the
/// stand-in listens on TCP only) and the HTTP/2 leg wins the race.
#[test]
fn quick_tunnel_standin_falls_back_to_http2_when_quic_has_no_listener() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (mut host, events) = standin.host(true, None);
    host.settings.udp = pod_cloudflare::udp::Choice::On;
    let config = pod_cloudflare::config::Config { protocol: Protocol::Auto, ..standin.config(Service::HelloWorld) };
    let handle = standin.run(host, config).expect("the tunnel starts");
    assert_eq!(handle.leg(), "http2");
    assert!(said(&events, "the edge leg http2 won"), "{:?}", events.lock().unwrap());
    standin.block_on(handle.shutdown());
}

#[test]
fn quick_tunnel_standin_edge_certificate_needs_its_ca() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    // Without the roots of the test, the certificate of the stand-in edge is
    // not valid: the crate adds no root for a test, and the registration
    // fails.
    let (host, _) = standin.host(false, None);
    assert!(standin.run(host, standin.config(Service::HelloWorld)).is_err());
    assert!(standin.edge.registrations.lock().unwrap().is_empty(), "the edge got a registration");
}

/// UDP 7844 dropped with no answer: a UDP socket on the port of the edge
/// takes each QUIC packet and answers none. The protocol auto registers
/// over HTTP/2 after the stagger, and an event names the leg that won.
#[test]
fn quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let silent = std::net::UdpSocket::bind(standin.edge_addr).expect("a UDP socket on the edge port");
    let (mut host, events) = standin.host(true, None);
    host.settings.udp = pod_cloudflare::udp::Choice::On;
    let config = pod_cloudflare::config::Config { protocol: Protocol::Auto, ..standin.config(Service::HelloWorld) };
    let handle = standin.run(host, config).expect("the tunnel starts");
    assert_eq!(handle.leg(), "http2");
    assert!(said(&events, "the edge leg http2 won"), "{:?}", events.lock().unwrap());
    // The QUIC leg sent its packets to the silent socket.
    silent.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let mut buf = [0u8; 2048];
    assert!(silent.recv_from(&mut buf).is_ok(), "no QUIC packet came");
    standin.block_on(handle.shutdown());
}

/// A proxy at the standard input and output of a command: the proxy
/// `http+exec:COMMAND` of `pod-proxy`, one command for each connection.
/// The command is a pipe in Python from its standard streams to the
/// CONNECT proxy of the stand-in. The service and the edge go through it.
#[test]
fn quick_tunnel_standin_goes_through_an_exec_proxy() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (proxy, log) = standin.proxy();
    let pipe = standin.dir.path().join("pipe.py");
    std::fs::write(
        &pipe,
        "import socket, sys, threading\n\
         s = socket.create_connection((sys.argv[1], int(sys.argv[2])))\n\
         def up():\n\
         \x20   while True:\n\
         \x20       b = sys.stdin.buffer.raw.read(65536)\n\
         \x20       if not b:\n\
         \x20           s.shutdown(socket.SHUT_WR)\n\
         \x20           return\n\
         \x20       s.sendall(b)\n\
         threading.Thread(target=up, daemon=True).start()\n\
         while True:\n\
         \x20   b = s.recv(65536)\n\
         \x20   if not b:\n\
         \x20       break\n\
         \x20   sys.stdout.buffer.write(b)\n\
         \x20   sys.stdout.buffer.flush()\n",
    )
    .unwrap();
    let spec = format!("http+exec:python3 -I {} {} {}", pipe.display(), proxy.ip(), proxy.port());
    let proxy = pod_proxy::Proxy::parse_trusted(&spec).expect("an exec proxy");
    let dial = standin::proxied(pod_proxy::ProxyChoice::Via(proxy));
    let (mut host, _) = standin.host(true, Some(dial));
    host.settings.udp = pod_cloudflare::udp::Choice::On;
    let config = pod_cloudflare::config::Config { protocol: Protocol::Auto, ..standin.config(Service::HelloWorld) };
    let handle = standin.run(host, config).expect("the tunnel starts");
    let name = hostname_of(handle.url());
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));
    let seen = log.lock().unwrap().clone();
    let api = standin.api_url.trim_start_matches("http://").to_string();
    assert!(seen.contains(&api), "{seen:?}");
    assert!(seen.contains(&standin.edge_addr.to_string()), "{seen:?}");
    standin.block_on(handle.shutdown());
}

/// The consumer gives its own counters with its own text. The metrics
/// service and the handle use them.
#[test]
fn quick_tunnel_standin_serves_the_metrics_of_the_consumer() {
    pod_testkit::need_loopback!();
    let standin = Standin::start();
    let (host, _) = standin.host(true, None);
    let extra: pod_cloudflare::metrics::Extra = std::sync::Arc::new(|| "consumer_decisions_total 7\n".to_string());
    let metrics = pod_cloudflare::metrics::Metrics::with_prefix("consumer").with_extra(extra);
    let given = metrics.clone();
    let handle = standin
        .run_with(host, standin.config(Service::Metrics), move |builder| builder.metrics(given))
        .expect("the tunnel starts");
    let name = hostname_of(handle.url());
    assert!(standin.block_on(standin.edge.wait_registered(&name, Duration::from_secs(20))));
    let (status, _, body) = standin.visit(&name, "/metrics", &[]);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("consumer_streams_total 1"), "{body}");
    assert!(body.contains("consumer_decisions_total 7"), "{body}");
    assert!(metrics.snapshot().connections >= 1);
    assert_eq!(handle.metrics().snapshot().streams_total, metrics.snapshot().streams_total);
    standin.block_on(handle.shutdown());
}
