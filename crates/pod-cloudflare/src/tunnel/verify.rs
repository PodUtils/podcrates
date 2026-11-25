//! The verify of the public URL of a tunnel: fetch it until it answers.

use std::time::Duration;

use crate::fail::{Failure, Result};
use pod_core::{Class, Event, Stage};

use crate::Host;

/// The loop of `TunnelHandle::verify_accepting`, with the budget and the
/// interval as arguments (for the tests).
pub(super) async fn verify_fetch(
    host: &Host,
    url: &str,
    expected: Option<&str>,
    accepted: &[u16],
    budget: Duration,
    interval: Duration,
    strict: bool,
) -> Result<Verified> {
    let mut last;
    let mut last_status;
    // Each attempt so far failed before the edge: at name resolution, or at
    // the CONNECT of the proxy.
    let mut resolution_only = true;
    let deadline = tokio::time::Instant::now() + budget;
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let request = pod_proxy::client::Request::new("GET", url).timeout(Duration::from_secs(15));
        match host.send(request).await {
            Ok(response) => {
                resolution_only = false;
                let body = response.text();
                let ok = response.is_success() && expected.map(|token| body.contains(token)).unwrap_or(true);
                if ok {
                    let dns = public_dns(host, url).await;
                    let text = format!("verified {url}: HTTP {} at attempt {attempt}, DNS step {dns}", response.status);
                    host.event(Event::info("tunnel.verify", text));
                    if dns == "doh" {
                        host.event(Event::info(
                            "tunnel.verify",
                            "the system resolver of this host does not resolve the new name yet; \
                             public DNS does. A visitor whose resolver cached NXDOMAIN gets an \
                             error for some seconds more",
                        ));
                    }
                    return Ok(Verified { dns, unverified: None });
                }
                last = format!("HTTP {}", response.status);
                last_status = Some(response.status);
                // A status of the origin crossed the tunnel: the tunnel is up.
                let edge_error = matches!(response.status, 502..=504 | 520..=530);
                if expected.is_none() && !edge_error {
                    if accepted.contains(&response.status) {
                        let text = format!(
                            "{url}: the origin answered HTTP {}, its normal answer; the tunnel is up",
                            response.status
                        );
                        host.event(Event::info("tunnel.verify", text));
                    } else {
                        let text = format!("{url}: the origin answered HTTP {}; the tunnel is up", response.status);
                        host.event(Event::warn("tunnel.verify", text));
                    }
                    return Ok(Verified { dns: public_dns(host, url).await, unverified: None });
                }
            }
            Err(err) => {
                last_status = None;
                if is_resolution_failure(&err) {
                    last = format!("waiting for DNS: {}", err.chain());
                    if attempt == 1 && crate::fail::stage_of(&err) == Some(Stage::Dns) {
                        let text =
                            format!("this host does not resolve the new name yet; {}", host.settings.words.resolve);
                        host.event(Event::info("tunnel.verify", text));
                    }
                } else {
                    resolution_only = false;
                    last = err.chain();
                }
            }
        }
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        // The user sees the retries: a new name can need a minute, and a
        // check with no line looks like a hang.
        let left_s = left.as_secs();
        if attempt > 10 && attempt.is_multiple_of(5) {
            let text = format!("the public URL does not answer yet ({last}); the check runs again for {left_s} s");
            host.event(Event::warn("tunnel.verify", text));
        } else if attempt == 1 || attempt.is_multiple_of(5) {
            let text = format!(
                "the public URL does not answer yet ({last}; a new name takes some seconds); the check runs again for {left_s} s"
            );
            host.event(Event::info("tunnel.verify", text));
        } else {
            tracing::debug!(attempt, %last, "verification retry");
        }
        tokio::time::sleep(interval.min(left)).await;
    }
    if expected.is_none() && matches!(last_status, Some(s) if s != 530) {
        host.event(Event::warn("tunnel.verify", format!("{url} answers {last}; the tunnel is up, check the origin")));
        return Ok(Verified { dns: public_dns(host, url).await, unverified: None });
    }
    if resolution_only && !strict {
        let text = format!(
            "the tunnel is registered; this host cannot reach {url} yet ({last}); the URL is not proved. {}",
            host.settings.words.strict
        );
        host.event(Event::warn("tunnel.verify", text));
        return Ok(Verified { dns: "none", unverified: Some("resolution") });
    }
    let hint = if resolution_only { format!("; {}", host.settings.words.resolve) } else { String::new() };
    let stage = if resolution_only { Stage::Dns } else { Stage::EdgeStream };
    Err(crate::fail::new(
        stage,
        Class::Unavailable,
        format!("could not verify {url} in {} s: {last}{hint}", budget.as_secs()),
    ))
}

/// The time of the retries of the verify fetch: a new name can need more
/// than a minute in public DNS.
pub(super) const VERIFY_BUDGET: Duration = Duration::from_secs(180);
/// The pause between two verify fetches.
pub(super) const VERIFY_INTERVAL: Duration = Duration::from_secs(2);

/// True for a failure before the edge: name resolution, or a refusal of
/// CONNECT by a proxy (a proxy that is not allowed to reach the name says
/// the same). The tunnel can be up in both cases.
fn is_resolution_failure(err: &Failure) -> bool {
    crate::fail::stage_of(err) == Some(Stage::Dns)
        || matches!(crate::fail::find::<pod_proxy::DialError>(err), Some(pod_proxy::DialError::ProxyRefused { .. }))
}

/// The result of a verify of the public URL.
#[derive(Clone, Copy, Debug)]
pub struct Verified {
    /// `Some("resolution")` when the verify did not reach the edge because
    /// this host cannot resolve the name, or a proxy refused it. The tunnel
    /// is up; the URL is not proved.
    pub unverified: Option<&'static str>,
    /// The step of the resolver chain that resolves the public name now:
    /// `literal`, `pin`, `system`, `doh` or `none`
    /// ([`Host::answering_step`]).
    pub dns: &'static str,
}

/// [`Host::answering_step`] for the host of `url`, in at most 10 s.
async fn public_dns(resolver: &Host, url: &str) -> &'static str {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', ':']).next().unwrap_or("");
    if host.is_empty() {
        return "none";
    }
    tokio::time::timeout(Duration::from_secs(10), resolver.answering_step(host)).await.unwrap_or("none")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local HTTP origin: answer N of `statuses` gives the Nth status,
    /// then the last one again.
    async fn origin(statuses: Vec<u16>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut n = 0;
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    return;
                };
                let status = statuses[n.min(statuses.len() - 1)];
                n += 1;
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let reply = format!("HTTP/1.1 {status} X\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
                let _ = s.write_all(reply.as_bytes()).await;
            }
        });
        format!("http://{addr}/")
    }

    // The retries go on for a time budget, not for a count.
    #[tokio::test]
    async fn verify_waits_through_edge_errors() {
        pod_testkit::need_loopback!();
        let url = origin(vec![530, 530, 502, 200]).await;
        let ok = verify_fetch(
            &crate::testing::host(),
            &url,
            Some("ok"),
            &[],
            Duration::from_secs(10),
            Duration::from_millis(20),
            false,
        )
        .await;
        assert!(ok.is_ok(), "{ok:?}");
    }

    // A verify that fails exits 69 (unavailable), not 1.
    #[tokio::test]
    async fn a_failed_verify_exits_69_after_the_budget() {
        pod_testkit::need_loopback!();
        let url = origin(vec![530]).await;
        let start = std::time::Instant::now();
        let err = verify_fetch(
            &crate::testing::host(),
            &url,
            None,
            &[],
            Duration::from_millis(300),
            Duration::from_millis(50),
            false,
        )
        .await
        .unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(crate::fail::class_of(&err).map(Class::exit_code), Some(69), "{err:#}");
        assert!(format!("{err:#}").contains("HTTP 530"), "{err:#}");
    }

    // A 404 of `serve` with no index.html is its normal answer.
    #[tokio::test]
    async fn an_accepted_status_of_the_origin_verifies() {
        pod_testkit::need_loopback!();
        let url = origin(vec![404]).await;
        let ok = verify_fetch(
            &crate::testing::host(),
            &url,
            None,
            &[404],
            Duration::from_secs(5),
            Duration::from_millis(20),
            false,
        )
        .await;
        assert!(ok.is_ok(), "{ok:?}");
    }

    /// The host of the URL goes to the resolver chain; an
    /// address needs no lookup, and a URL with no host is "none".
    #[tokio::test]
    async fn the_dns_step_of_a_url() {
        pod_testkit::need_localhost!();
        let host = crate::testing::host();
        assert_eq!(public_dns(&host, "https://127.0.0.1:8443/s/x/").await, "literal");
        assert_eq!(public_dns(&host, "https://localhost/").await, "system");
        assert_eq!(public_dns(&host, "https:///x").await, "none");
    }

    // A name that this host cannot resolve keeps the tunnel up.
    #[tokio::test]
    async fn a_name_that_does_not_resolve_keeps_the_tunnel_up() {
        if crate::testing::host().proxy_for("podnet-no-such-name.invalid", 80).is_some() {
            // A proxy of this host answers for the name.
            return;
        }
        let url = "http://podnet-no-such-name.invalid/";
        let ok = verify_fetch(
            &crate::testing::host(),
            url,
            None,
            &[],
            Duration::from_millis(300),
            Duration::from_millis(50),
            false,
        )
        .await
        .unwrap();
        assert_eq!(ok.unverified, Some("resolution"));
        assert_eq!(ok.dns, "none");
        // A body token (the demo) does not change it.
        let ok = verify_fetch(
            &crate::testing::host(),
            url,
            Some("x"),
            &[],
            Duration::from_millis(300),
            Duration::from_millis(50),
            false,
        )
        .await
        .unwrap();
        assert_eq!(ok.unverified, Some("resolution"));
    }

    // The strict switch gives exit 69, and the error names the remedy.
    #[tokio::test]
    async fn verify_failure_names_the_remedy() {
        if crate::testing::host().proxy_for("podnet-no-such-name.invalid", 80).is_some() {
            // A proxy of this host answers for the name.
            return;
        }
        let err = verify_fetch(
            &crate::testing::host(),
            "http://podnet-no-such-name.invalid/",
            None,
            &[],
            Duration::from_millis(300),
            Duration::from_millis(50),
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(crate::fail::class_of(&err).map(Class::exit_code), Some(69), "{err:#}");
        assert!(format!("{err:#}").contains("--resolve HOST=IP"), "{err:#}");
        assert!(format!("{err:#}").contains("--no-verify"), "{err:#}");
    }

    // A refusal of CONNECT by a proxy is a failure before the edge.
    #[test]
    fn a_proxy_refusal_is_a_resolution_failure() {
        let refusal = Failure::from(pod_proxy::DialError::ProxyRefused {
            proxy: "http://127.0.0.1:3128".into(),
            target: "x.example:443".into(),
            status: 502,
            reason: "refused".into(),
        });
        assert!(is_resolution_failure(&refusal));
        assert!(!is_resolution_failure(&crate::fail::new(Stage::Network, Class::Unavailable, "reset")));
    }
}
