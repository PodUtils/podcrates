//! The rule, the visitor and the faults of the capture sink
//! (`crate::capture::{Route, Fault}`) with a stand-in origin.

use std::sync::{Arc, Mutex};

use super::capture_tests::{origin, request, runtime, visit};
use super::*;
use crate::capture::{Capture, Direction, End, Fault, Head, Refused, Route};
use crate::ingress::IngressRule;

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Route(Route),
    Fault(Fault),
    Head(Head),
    Body(Direction, Vec<u8>),
    End(End),
}

#[derive(Default)]
struct Log(Mutex<Vec<Call>>);

impl Log {
    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl Capture for Log {
    fn route(&self, _: u64, route: &Route) -> Result<(), Refused> {
        self.0.lock().unwrap().push(Call::Route(route.clone()));
        Ok(())
    }
    fn fault(&self, _: u64, fault: Fault) {
        self.0.lock().unwrap().push(Call::Fault(fault));
    }
    fn head(&self, _: u64, head: &Head) -> Result<(), Refused> {
        self.0.lock().unwrap().push(Call::Head(head.clone()));
        Ok(())
    }
    fn body(&self, _: u64, direction: Direction, bytes: &[u8]) -> Result<(), Refused> {
        self.0.lock().unwrap().push(Call::Body(direction, bytes.to_vec()));
        Ok(())
    }
    fn end(&self, _: u64, end: &End) {
        self.0.lock().unwrap().push(Call::End(end.clone()));
    }
}

/// The sink gets the index and the hostname of the rule, and the `Host` of
/// the visitor, before the head; two rules with the same `httpHostHeader`
/// stay apart.
#[tokio::test]
async fn the_sink_gets_the_rule_and_the_visitor() {
    pod_testkit::need_loopback!();
    let at = origin().await;
    let same = |hostname: &str| {
        let mut rule = IngressRule::catch_all(Service::Http(at.clone()));
        rule.hostname = Some(hostname.to_string());
        rule.origin.http_host_header = Some("same.internal".to_string());
        rule
    };
    let ingress =
        Ingress::new().rule_with(same("a.example")).rule_with(same("b.example")).catch_all(Service::Http(at.clone()));
    let log = Arc::new(Log::default());
    let runtime = runtime(&ingress, Some(log.clone()));
    for (host, rule, hostname) in
        [("b.example", 1, Some("b.example")), ("a.example", 0, Some("a.example")), ("c", 2, None)]
    {
        let mut req = request("/chunked", &[], false);
        req.metadata.retain(|(key, _)| key != HTTP_HOST_KEY);
        req.metadata.push((HTTP_HOST_KEY.into(), host.into()));
        visit(&runtime, &ingress, req, b"").await;
        let calls = log.take();
        let want = Route { rule, hostname: hostname.map(str::to_string), host: host.to_string() };
        assert_eq!(calls.first(), Some(&Call::Route(want)), "{host}: {calls:?}");
        let Some(Call::Head(head)) = calls.get(1) else { panic!("{host}: the head follows the route: {calls:?}") };
        let sent = if hostname.is_some() { "same.internal" } else { host };
        assert_eq!(head.header("host"), Some(sent), "{host}");
    }
}

/// A chunk size that the decoder of the crate refuses gives one fault and
/// one end, no more of that body, and the bytes of the visitor stay the
/// same as with no sink.
#[tokio::test]
async fn a_refused_chunk_body_still_gets_its_end() {
    pod_testkit::need_loopback!();
    let ingress = Ingress::new().catch_all(Service::Http(origin().await));
    let log = Arc::new(Log::default());
    let sink = runtime(&ingress, Some(log.clone()));
    let plain = runtime(&ingress, None);
    let extra = [("Transfer-Encoding", "chunked")];
    for sent in [&b"ffffffffffffffff\r\nab"[..], b"2\r\nok\r\nffffffffffffffff\r\ncd", b"zz\r\nab"] {
        let want = visit(&plain, &ingress, request("/upload", &extra, false), sent).await;
        let got = visit(&sink, &ingress, request("/upload", &extra, false), sent).await;
        assert_eq!(got, want, "{:?}", String::from_utf8_lossy(sent));
        let calls = log.take();
        let ends = calls.iter().filter(|c| matches!(c, Call::End(_))).count();
        assert_eq!(ends, 1, "{calls:?}");
        assert!(matches!(calls.last(), Some(Call::End(_))), "{calls:?}");
        let n = calls.len();
        assert_eq!(calls[n - 2], Call::Fault(Fault::RequestChunking), "{calls:?}");
        let up: Vec<u8> = calls
            .iter()
            .filter_map(|c| if let Call::Body(Direction::Request, b) = c { Some(b.clone()) } else { None })
            .flatten()
            .collect();
        assert!(up == b"" || up == b"ok", "the sink gets no byte of a body after the fault: {up:?}");
    }
}
