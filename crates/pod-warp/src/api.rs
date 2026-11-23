//! The WARP API: its bases in the order that they are tried, the headers of
//! the official client, and the send of one request through the HTTP
//! client of the caller (`pod_proxy::Client`: its dial, its proxy, its
//! roots).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use pod_proxy::Client;
use pod_proxy::client::Request;
use pod_proxy::http::Response;

use crate::service;

/// The time limit of one API request.
pub const API_TIMEOUT: Duration = Duration::from_secs(30);

/// A WARP API answer that is not a success: its HTTP status and body. It is
/// the cause of the [`Failure`] of the call ([`api_status`] reads it).
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub body: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WARP API answered HTTP {}: {}", self.status, self.body)
    }
}

impl std::error::Error for ApiError {}

/// The class of an answer of the API with `status`.
fn class_of(status: u16) -> Class {
    match status {
        429 => Class::TempFail,
        401 | 403 => Class::Refused,
        404 => Class::NotFound,
        400..=499 => Class::Protocol,
        _ => Class::Unavailable,
    }
}

impl ApiError {
    /// The failure of `what` with this answer as its cause.
    pub fn failure(self, what: &str) -> Failure {
        let class = class_of(self.status);
        Failure::caused(Stage::Warp, class, format!("{what}: {self}"), self)
    }
}

/// The HTTP status of a WARP API answer in the chain of `failure`.
pub fn api_status(failure: &Failure) -> Option<u16> {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(failure);
    while let Some(e) = next {
        if let Some(api) = e.downcast_ref::<ApiError>() {
            return Some(api.status);
        }
        next = e.source();
    }
    None
}

/// The token of a team (a Cloudflare One organization), which a
/// registration sends. Its value never goes into a log line.
#[derive(Clone)]
struct Team(zeroize::Zeroizing<String>);

impl std::fmt::Debug for Team {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Team(..)")
    }
}

/// The API of a caller: its HTTP client, the bases that it tries, the token
/// of a team, and the base that answered last. A clone shares the base that
/// answered.
#[derive(Clone, Debug)]
pub struct Api {
    client: Client,
    bases: Vec<String>,
    version: String,
    team: Option<Team>,
    answered: Arc<Mutex<Option<String>>>,
}

impl Api {
    /// The API through `client`, at [`service::api_base`] and then
    /// [`service::api_second`].
    pub fn new(client: Client) -> Api {
        Api {
            client,
            bases: vec![service::api_base(), service::api_second()],
            version: service::api_version().into(),
            team: None,
            answered: Arc::default(),
        }
    }

    /// The token of a team: a registration sends it as
    /// `CF-Access-Jwt-Assertion`, and the device is the team's (podssh's
    /// `PODSSH_WARP_TEAM_TOKEN_FILE`).
    pub fn with_team(mut self, token: &str) -> Api {
        self.team = Some(Team(zeroize::Zeroizing::new(token.to_string())));
        self
    }

    /// The token of a team, for the registration.
    pub(crate) fn team(&self) -> Option<&str> {
        self.team.as_ref().map(|t| t.0.as_str())
    }

    /// The base that gave the last answer under 500, so that a caller keeps
    /// it and tries it first next time ([`Api::with_bases`]).
    pub fn answered(&self) -> Option<String> {
        self.answered.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The one base `value` (`https://HOST[:PORT]`; `http://` for a
    /// loopback host only), as podnet's `PODNET_WARP_API` and podssh's
    /// `PODSSH_WARP_API` give it. A blank value keeps the bases.
    pub fn with_base(mut self, value: &str) -> Result<Api, Failure> {
        self.bases = bases(Some(value))?;
        Ok(self)
    }

    /// The bases `bases`, tried in this order (podssh's routes of the API).
    pub fn with_bases(mut self, bases: Vec<String>) -> Api {
        self.bases = bases;
        self
    }

    /// The bases in the order that they are tried.
    pub fn bases(&self) -> &[String] {
        &self.bases
    }

    /// A request of the API at `path` (after the version), with the headers
    /// of the official client and the token `bearer`.
    pub fn request(&self, method: &'static str, path: &str, bearer: Option<&str>) -> Request {
        let mut request = Request::new(method, format!("{}/{}{path}", service::api_base(), self.version))
            .header("User-Agent", service::user_agent())
            .header("CF-Client-Version", service::client_version())
            .header("Content-Type", "application/json; charset=UTF-8")
            .timeout(API_TIMEOUT);
        if let Some(token) = bearer {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request
    }

    /// Send `request` (made for [`service::api_base`]) to each base in turn,
    /// until one answers with a status under 500. The failure names each
    /// base once (`each_failed`). A `POST` goes on only after a failure
    /// with no answer: any answer of the service is final
    /// (PodUtils/podnet@T-241), as it may have made a device.
    pub async fn send(&self, request: Request) -> Result<Response, Failure> {
        let base = service::api_base();
        let path = request.url.strip_prefix(base.as_str()).unwrap_or(&request.url).to_string();
        let mut failed: Vec<(String, Result<u16, Failure>)> = Vec::new();
        let count = self.bases.len();
        for (i, base) in self.bases.iter().enumerate() {
            let mut attempt = request.clone();
            attempt.url = format!("{base}{path}");
            let last = i + 1 == count;
            match self.client.send(attempt).await {
                Ok(response) if response.status < 500 || last || request.method == "POST" => {
                    if response.status < 500 {
                        *self.answered.lock().unwrap_or_else(PoisonError::into_inner) = Some(base.clone());
                    }
                    return Ok(response);
                }
                Ok(response) => failed.push((base.clone(), Ok(response.status))),
                Err(err) => failed.push((base.clone(), Err(err))),
            }
            if !last && let Some((base, result)) = failed.last() {
                let said = result.as_ref().map_or_else(Failure::chain, |status| format!("{base}: HTTP {status}"));
                tracing::info!(failed = %said, "the WARP API: the next host is tried");
            }
        }
        Err(each_failed(failed))
    }
}

/// The failure of the API when no base answered: each base with its HTTP
/// status or its failure. The bases that failed at the same hop (a proxy
/// that does not answer) are one entry that names the hop once
/// ([`pod_proxy::group_texts`]); the failure of a base that shares no hop
/// names its request, and thus its base, once.
fn each_failed(mut failed: Vec<(String, Result<u16, Failure>)>) -> Failure {
    let failures: Vec<&Failure> = failed.iter().filter_map(|(_, r)| r.as_ref().err()).collect();
    let hop = if failures.len() > 1 { shared_hop(&failures) } else { None };
    let entries: Vec<(String, String)> = failed
        .iter()
        .map(|(base, result)| match (result, &hop) {
            (Ok(status), _) => (base.clone(), format!("HTTP {status}")),
            (Err(_), Some((text, _))) => (base.clone(), text.clone()),
            (Err(failure), None) => (String::new(), failure.chain()),
        })
        .collect();
    let text = match (&hop, entries.as_slice()) {
        (None, [(base, one)]) if base.is_empty() => pod_core::failure::lead("the WARP API", one),
        (Some(_), _) => format!("the WARP API: each host failed: {}", pod_proxy::group_texts(&entries, "hosts")),
        (None, _) => {
            let said: Vec<String> = entries
                .iter()
                .map(|(base, text)| if base.is_empty() { text.clone() } else { format!("{base}: {text}") })
                .collect();
            format!("the WARP API: each host failed: {}", said.join("; "))
        }
    };
    let last = match failed.pop() {
        Some((_, Err(last))) => last,
        _ => return Failure::new(Stage::Warp, Class::Usage, "the WARP API has no base"),
    };
    let (stage, class) = (last.stage, last.class);
    match hop {
        Some((_, depth)) => Failure::caused(stage, class, text, EachHost { last, depth }),
        None => Failure::caused(stage, class, text, last),
    }
}

/// The hop that each of `failures` failed at: the outer [`Failure`] or
/// [`pod_proxy::DialError`] of the chain of the last failure whose text
/// (with its causes) is in the chain of each other failure. Gives the text
/// and its depth in the chain of the last failure. An error of I/O alone
/// (`Connection refused`) is no hop: its address is in the words above it.
fn shared_hop(failures: &[&Failure]) -> Option<(String, usize)> {
    let hops = |failure: &Failure| {
        let mut out = Vec::new();
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(failure);
        let mut depth = 0;
        while let Some(e) = next {
            if e.is::<Failure>() || e.is::<pod_proxy::DialError>() {
                out.push((pod_core::failure::chain_of(e), depth));
            }
            next = e.source();
            depth += 1;
        }
        out
    };
    let (last, others) = failures.split_last()?;
    let theirs: Vec<Vec<(String, usize)>> = others.iter().map(|f| hops(f)).collect();
    hops(last).into_iter().find(|(text, _)| theirs.iter().all(|hops| hops.iter().any(|(t, _)| t == text)))
}

/// The cause of a failure of each host at one hop: the hop in the chain of
/// the last failure, so that a caller reads its `DialError` and its cut. Its
/// own words are in the text of the failure already.
#[derive(Debug)]
struct EachHost {
    last: Failure,
    depth: usize,
}

impl EachHost {
    fn hop(&self) -> &(dyn std::error::Error + 'static) {
        let mut hop: &(dyn std::error::Error + 'static) = &self.last;
        for _ in 0..self.depth {
            match hop.source() {
                Some(next) => hop = next,
                None => break,
            }
        }
        hop
    }
}

impl std::fmt::Display for EachHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("each host failed")
    }
}

impl std::error::Error for EachHost {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.hop())
    }
}

/// The bases of a value: [`service::api_base`] and [`service::api_second`]
/// for none or a blank value, else the one base that the value names.
pub fn bases(value: Option<&str>) -> Result<Vec<String>, Failure> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(vec![service::api_base(), service::api_second()]);
    };
    let bad = |what: &str| Failure::new(Stage::Warp, Class::Config, format!("the WARP API {value:?}: {what}"));
    let value = value.trim_end_matches('/');
    let rest = value
        .strip_prefix("https://")
        .or_else(|| {
            value.strip_prefix("http://").filter(|r| r.starts_with("127.0.0.1:") || r.starts_with("localhost:"))
        })
        .ok_or_else(|| bad("use https://HOST[:PORT] (http:// for a loopback host only)"))?;
    if rest.is_empty() || rest.contains(['/', '?', '@', '#']) {
        return Err(bad("use https://HOST[:PORT], with no path"));
    }
    Ok(vec![value.to_string()])
}

/// The answer `response` as a success, or the [`ApiError`] of `what`.
pub(crate) fn success(response: Response, what: &str) -> Result<Response, Failure> {
    if response.is_success() {
        return Ok(response);
    }
    Err(ApiError { status: response.status, body: response.body_text(200) }.failure(what))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_bases() {
        assert_eq!(bases(None).unwrap(), [service::api_base(), service::api_second()]);
        assert_eq!(bases(Some(" ")).unwrap(), [service::api_base(), service::api_second()]);
        assert_eq!(bases(Some("https://api.devices.cloudflare.com/")).unwrap(), [service::api_second()]);
        assert_eq!(bases(Some("http://127.0.0.1:8080")).unwrap(), ["http://127.0.0.1:8080"]);
        for bad in [
            "http://api.cloudflareclient.com",
            "api.cloudflareclient.com",
            "https://api.cloudflareclient.com/v0",
            "https://",
        ] {
            let err = bases(Some(bad)).unwrap_err();
            assert_eq!(err.class, Class::Config, "{bad}");
        }
    }

    /// The words of a dialer of the caller over a `DialError`, as podnet's
    /// context of anyhow gives them.
    #[derive(Debug)]
    struct Through(String, pod_proxy::DialError);

    impl std::fmt::Display for Through {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for Through {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.1)
        }
    }

    /// The failure of `base` through a proxy that does not answer, in the
    /// form of podnet's dialer under `pod_proxy::Client::send`.
    fn through_dead_proxy(host: &str) -> Failure {
        let proxy = "127.0.0.1:1";
        let detail = "could not connect to 127.0.0.1:1: Connection refused (os error 111)".to_string();
        let dial = pod_proxy::DialError::ProxyUnreachable { proxy: proxy.into(), detail };
        let words = format!("{host}:443 through the proxy http://{proxy}");
        let text = format!("{words}: {dial}");
        let dialer = Failure::caused(Stage::Proxy, Class::Unavailable, text, Through(words, dial));
        let request = format!("POST https://{host}/v0a5641/reg: {}", dialer.text());
        Failure::caused(Stage::Proxy, Class::Unavailable, request, dialer)
    }

    /// The hop that each base failed at is said once, and stays the cause
    /// (PodUtils/podnet#658).
    #[test]
    fn each_base_failed_at_one_hop_names_it_once() {
        let failed = vec![
            ("https://a.example".to_string(), Err(through_dead_proxy("a.example"))),
            ("https://b.example".to_string(), Err(through_dead_proxy("b.example"))),
        ];
        let failure = each_failed(failed);
        let chain = failure.chain();
        assert_eq!(
            chain,
            "the WARP API: each host failed: 2 hosts (https://a.example, https://b.example): could not reach \
             the proxy 127.0.0.1:1: Connection refused (os error 111)"
        );
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut found = false;
        while let Some(e) = next {
            found |= e.is::<pod_proxy::DialError>();
            next = e.source();
        }
        assert!(found, "the DialError stays in the chain");
        assert_eq!((failure.stage, failure.class), (Stage::Proxy, Class::Unavailable));
        let one = each_failed(vec![("https://a.example".to_string(), Err(through_dead_proxy("a.example")))]);
        assert_eq!(
            one.chain().matches("a.example").count(),
            2,
            "the request and the host of the proxy: {}",
            one.chain()
        );
        assert!(one.chain().starts_with("the WARP API: POST https://a.example/"), "{}", one.chain());
    }

    #[test]
    fn the_status_of_an_answer_is_in_the_chain() {
        let failure = ApiError { status: 429, body: "error code: 1015".into() }.failure("POST /reg");
        assert_eq!(api_status(&failure), Some(429));
        assert_eq!(failure.class, Class::TempFail);
        let wrapped = Failure::caused(Stage::Warp, Class::Unavailable, "registering", failure);
        assert_eq!(api_status(&wrapped), Some(429));
        assert_eq!(api_status(&Failure::new(Stage::Warp, Class::Unavailable, "no route")), None);
    }
}
