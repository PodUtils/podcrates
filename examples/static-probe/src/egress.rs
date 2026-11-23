//! pod-egress: a dialer of each road whose deadline is now, so that each
//! road links in and nothing connects; the arguments of a meek_lite and a
//! WebTunnel bridge, the upgrade of the Access road, a presigned path of
//! EICE, and the path of IAP with the carrier's first link refused. VLESS,
//! VMess, Trojan and Shadowsocks are dialers as the roads of part 1.

use std::sync::Arc;
use std::time::Duration;

use pod_core::{Dial, Target};
use pod_egress::{AtEnd, Note, forwarder, shadowsocks, trojan, via, vless, vmess, wstunnel};
use pod_tls::Trust;

pub fn egress() -> String {
    let provider = Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe")));
    let settings = pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::Direct);
    let dialer = pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()));
    let way = pod_ws::Way::new(Arc::new(tls), Arc::new(dialer), "static-probe");
    let wait = Duration::from_secs(1);
    let note: Note = Arc::new(pod_core::NoEvents);
    let mut dialers: Vec<Box<dyn Dial>> = Vec::new();
    if let Ok(proxy) = via::Via::parse("https://proxy.example") {
        let dial = via::Dial {
            way: way.clone(),
            trust: Trust::Default,
            timeout: wait,
            offer: via::Offer::Both,
            auth_file: "a file".into(),
        };
        dialers.push(Box::new(via::Client::new(proxy, dial)));
    }
    let builtin = forwarder::builtin::builtin();
    if let Some(template) = builtin.first().and_then(|b| forwarder::Template::parse(b.template).ok()) {
        let dial = forwarder::Dial {
            way: way.clone(),
            trust: Trust::Default,
            timeout: wait,
            at_end: AtEnd::Now,
            note: note.clone(),
        };
        dialers.push(Box::new(forwarder::Forwarder { template, dial }));
    }
    if let Ok(server) = wstunnel::Server::parse("wss://wstunnel.example") {
        let dial = wstunnel::Dial { way: way.clone(), trust: Trust::Default, timeout: wait, at_end: AtEnd::Now };
        dialers.push(Box::new(wstunnel::Tunnel { server, secret: wstunnel::Secret::default(), dial }));
    }
    part2(&way, wait, &mut dialers);
    let target = Target::new("probe.example", 22);
    let dialed = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => dialers
            .iter()
            .map(|d| {
                let now = tokio::time::Instant::now();
                let done =
                    runtime.block_on(d.dial(&target, now)).map_or_else(|e| e.class.to_string(), |_| "open".into());
                format!("{} {done}", d.label())
            })
            .collect::<Vec<_>>()
            .join(", "),
        Err(e) => e.to_string(),
    };
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    let meek = pod_egress::meek::Args::parse(&[pair("url", "https://meek.example/"), pair("front", "front.example")])
        .map_or_else(|e| e, |_| "meek_lite".into());
    let webtunnel = pod_egress::webtunnel::Args::parse(&[pair("url", "https://webtunnel.example/secret")])
        .map_or_else(|e| e, |a| a.shown());
    let access = pod_egress::cloudflare::Access::default();
    let upgrade =
        pod_egress::cloudflare::request("tunnel.example", &access, "AAAAAAAAAAAAAAAAAAAAAA==", "static-probe")
            .map_or_else(|e| e, |r| format!("{} bytes", r.len()));
    let _ = pod_egress::wisp::PROTOCOL;
    let eice = eice();
    let iap = iap(&way);
    let _ = pod_egress::pt::Reach { way, trust: Trust::Default, wait };
    format!("pod-egress: {dialed}; {meek}, {webtunnel}, an upgrade of {upgrade}; {eice}; {iap}")
}

/// A path that SigV4 presigns, with a made-up key: its length.
fn eice() -> String {
    use pod_egress::eice::{Credentials, Endpoint, Target, presigned_path};
    let dns = "eice-0123456789example.b67b86ba.ec2-instance-connect-endpoint.us-west-2.amazonaws.com";
    let Ok(endpoint) = Endpoint::parse(dns) else { return "no endpoint".into() };
    let credentials = Credentials {
        access_key_id: "PROBE".to_string().into(),
        secret_access_key: "PROBE".to_string().into(),
        session_token: None,
    };
    let target = Target { address: std::net::Ipv4Addr::LOCALHOST.into(), port: 22, max_duration: None };
    format!("a presigned path of {} bytes", presigned_path(&endpoint, &target, &credentials, 0).len())
}

/// The path of a connect, and the carrier's first link, refused for its
/// empty token before it connects, so that the carrier links in.
fn iap(way: &pod_ws::Way) -> String {
    use pod_egress::iap::{self, Target, carry};
    let Ok(target) = Target::new("a-project", "us-central1-a", "a-vm", 22) else { return "no target".into() };
    let path = iap::path(&target, None);
    let (way, linked) = (way.clone(), target.clone());
    let dial: carry::Dial<pod_ws::url::Stream, pod_ws::client::ConnectError> = Arc::new(move |_| {
        let (way, target) = (way.clone(), linked.clone());
        Box::pin(async move { iap::connect(&target, None, "", &way, &Trust::Default, Duration::ZERO).await })
    });
    let opened = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime
            .block_on(carry::open(dial, Arc::new(pod_core::NoEvents)))
            .map_or_else(|e| e.to_string(), |_| "open".into()),
        Err(e) => e.to_string(),
    };
    format!("{path}: {opened}")
}

/// The roads of part 2, each a dialer with a made-up server.
fn part2(way: &pod_ws::Way, wait: Duration, dialers: &mut Vec<Box<dyn Dial>>) {
    let dial = || vless::Dial {
        way: way.clone(),
        trust: Trust::Default,
        timeout: wait,
        answer: wait,
        silence_is_ready: false,
        at_end: AtEnd::Now,
        early: None,
    };
    let id = "00000000-0000-4000-8000-000000000000";
    if let Ok(server) = vless::Server::parse(&format!("vless://{id}@vless.example:443?type=ws&security=tls&path=%2F")) {
        dialers.push(Box::new(vless::Road { server, dial: dial() }));
    }
    if let Ok(server) = vmess::Server::parse(
        "vmess://eyJ2IjoiMiIsInBzIjoicHJvYmUiLCJhZGQiOiJ2bWVzcy5leGFtcGxlIiwicG9ydCI6IjQ0MyIsImlkIjoiMDAwMDAwMDAtMDAwMC00MDAwLTgwMDAtMDAwMDAwMDAwMDAwIiwiYWlkIjoiMCIsInNjeSI6ImF1dG8iLCJuZXQiOiJ3cyIsInBhdGgiOiIvIiwidGxzIjoidGxzIn0=",
    ) {
        dialers.push(Box::new(vmess::Road { server, dial: dial() }));
    }
    if let Ok(server) = trojan::Server::parse("trojan://made-up@trojan.example:443?type=ws&path=%2F") {
        dialers.push(Box::new(trojan::Road { server, dial: dial() }));
    }
    if let Ok(node) = shadowsocks::Node::parse("ss://YWVzLTEyOC1nY206dGVzdA@192.0.2.1:8888") {
        let dial = shadowsocks::Dial {
            way: way.clone(),
            trust: Trust::Default,
            timeout: wait,
            answer: Some(wait),
            silence_is_ready: false,
        };
        dialers.push(Box::new(shadowsocks::Road { node, dial }));
    }
}
