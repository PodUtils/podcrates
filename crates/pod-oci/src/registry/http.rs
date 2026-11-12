//! One GET of the client: the challenge of the registry, the token, the
//! `Basic` answer from the logins, and the bounded retry around them.

use pod_core::Event;

use super::wire::{Attempt, backoff, host_of, key, parse_challenge, retryable, status_error, transport_error};
use super::*;
use crate::net::{query_value, text};

impl Client {
    /// One GET, with the challenge and the bounded retry around it.
    pub(super) fn get(&mut self, path: &str, endpoint: &str, scope: &str, accept: Option<&str>) -> Result<Body> {
        let mut last: Option<Failure> = None;
        let mut target = format!("{}{path}", self.base(endpoint));
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(backoff(attempt, None));
            }
            match self.get_once(&target, endpoint, scope, accept) {
                Ok(Attempt::Done(r)) => return Ok(*r),
                Ok(Attempt::Retry { after, failure, transport }) => {
                    // A transport failure of an endpoint that the caller
                    // named insecure is the one case of a new try over
                    // plain HTTP, and the client says so. Never a finding of
                    // its own: `after_connect_failure` gives `None` for each
                    // endpoint that nobody named.
                    if let Some(why) = &transport
                        && !self.http_endpoints.contains(endpoint)
                        && let Some(d) = self.policy.after_connect_failure(endpoint)
                    {
                        self.http_endpoints.insert(endpoint.to_string());
                        self.events.event(Event::warn(
                            "oci.plain-http",
                            format!(
                                "{endpoint} did not answer over HTTPS ({}), and it is configured as an \
                                 insecure registry, so this and every later request to it use {}://",
                                why, d.scheme
                            ),
                        ));
                        target = format!("{}{}", self.base(endpoint), path);
                        continue;
                    }
                    if attempt + 1 < ATTEMPTS {
                        std::thread::sleep(backoff(attempt + 1, after));
                    }
                    last = Some(failure);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| {
            http(
                Class::Unavailable,
                &format!("GET {}", redact(&target)),
                format!("gave up after {ATTEMPTS} attempt(s)"),
            )
        }))
    }

    fn get_once(&mut self, url: &str, endpoint: &str, scope: &str, accept: Option<&str>) -> Result<Attempt> {
        let verify = self.policy.verify(endpoint);
        match self.send(url, scope, accept, None, verify) {
            Ok(r) => Ok(Attempt::Done(Box::new(r))),
            Err(Refusal::Status(resp)) if resp.status() == 401 => {
                // No login and no token goes over plain HTTP: a registry
                // named insecure is one whose certificate is not checked,
                // not one that may read a password on the wire.
                if !url.starts_with("https://") {
                    return Err(http(
                        Class::Policy,
                        &format!("GET {}", redact(url)),
                        format!(
                            "{endpoint} asked for a login over plain HTTP, and the client sends no login over \
                             plain HTTP. Serve the registry over HTTPS: with the registry named insecure, its \
                             certificate is not checked"
                        ),
                    ));
                }
                // The challenge decides where the token comes from. It is
                // read from the answer, not from a table of hosts, so a
                // registry that is not Docker Hub works.
                let Some(challenge) = resp.header("WWW-Authenticate").map(str::to_string) else {
                    return Err(http(
                        Class::Protocol,
                        &format!("GET {}", redact(url)),
                        "401 with no WWW-Authenticate header, so there is no challenge to answer",
                    ));
                };
                // A `Basic` challenge gets the stored login, never a new
                // try with no login: it fails the same way.
                let scheme = challenge.split_whitespace().next().unwrap_or("");
                if scheme.eq_ignore_ascii_case("basic") {
                    return self.answer_basic(url, scope, accept, &challenge, verify);
                }
                let token = self.token(endpoint, scope, &challenge)?;
                self.tokens.insert(key(endpoint, scope), token);
                match self.send(url, scope, accept, None, verify) {
                    Ok(r) => Ok(Attempt::Done(Box::new(r))),
                    Err(e) => Err(status_error(url, e)),
                }
            }
            Err(Refusal::Status(resp)) if retryable(resp.status()) => {
                let after = resp.header("Retry-After").and_then(super::wire::parse_retry_after);
                Ok(Attempt::Retry { after, failure: status_error(url, Refusal::Status(resp)), transport: None })
            }
            Err(Refusal::Transport(t)) => {
                let why = t.why.clone();
                Ok(Attempt::Retry { after: None, failure: transport_error(url, t), transport: Some(why) })
            }
            Err(e) => Err(status_error(url, e)),
        }
    }

    /// One GET through the transport, with the trust that the policy gives
    /// the endpoint.
    fn send(
        &mut self,
        url: &str,
        scope: &str,
        accept: Option<&str>,
        basic: Option<&str>,
        verify: bool,
    ) -> std::result::Result<Body, Refusal> {
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(a) = accept {
            headers.push(("Accept", a.to_string()));
        }
        // The key is the host of the URL. `pod-proxy` sends no
        // `Authorization` header to the host of a redirect (a CDN), so the
        // token goes to the registry only.
        //
        // A `Basic` answer goes in place of the token, never beside it: the
        // 401 refused the token of the first request, and a refused
        // credential sent again beside the new one gets to the logs of
        // refused tries.
        if let Some(b) = basic {
            headers.push(("Authorization", b.to_string()));
        } else if let Some(host) = host_of(url)
            && let Some(t) = self.tokens.get(&key(&host, scope))
        {
            headers.push(("Authorization", format!("Bearer {t}")));
        }
        self.net.send("GET", url, &headers, verify, LIMITS)
    }

    /// Answer a `Basic` challenge from the stored logins. No login is a
    /// refusal by name (`Class::Refused`), not a new try with no login: the
    /// registry refused the request with no login. A failed helper gives
    /// its own failure.
    fn answer_basic(
        &mut self,
        url: &str,
        scope: &str,
        accept: Option<&str>,
        challenge: &str,
        verify: bool,
    ) -> Result<Attempt> {
        let host = host_of(url).unwrap_or_else(|| "<unknown>".into());
        let what = format!("GET {}", redact(url));
        let cred = match self.logins.lookup(&host) {
            Ok(Some(cred)) => cred,
            Ok(None) => {
                return Err(http(
                    Class::Refused,
                    &what,
                    format!(
                        "the registry asked for Basic authentication (challenge {challenge:?}), \
                         and no login is stored for {host}"
                    ),
                ));
            }
            Err(refused) => {
                return Err(Failure::caused(refused.stage, refused.class, format!("{what}: {refused}"), refused));
            }
        };
        let header = crate::credentials::basic_header(&cred);
        match self.send(url, scope, accept, Some(&header), verify) {
            Ok(r) => Ok(Attempt::Done(Box::new(r))),
            Err(e) => Err(status_error(url, e)),
        }
    }

    /// Answer a `Bearer` challenge at the realm that it names.
    fn token(&mut self, endpoint: &str, scope: &str, challenge: &str) -> Result<String> {
        let params = parse_challenge(challenge);
        let scheme = challenge.split_whitespace().next().unwrap_or("");
        let what = format!("authenticating to {endpoint}");
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(http(
                Class::Unsupported,
                &what,
                format!("the registry asked for {scheme:?} authentication, and the client implements Bearer only"),
            ));
        }
        let Some(realm) = params.get("realm") else {
            return Err(http(Class::Protocol, &what, "the Bearer challenge names no realm"));
        };
        if realm.starts_with("http://") {
            // The registry named a plain-HTTP realm: a refusal of the input
            // of somebody else, so a failure of the run, not of the use.
            return Err(http(
                Class::Policy,
                &what,
                format!(
                    "the token realm is {}, and the client speaks HTTPS only. Outbound tcp/80 \
                     is dropped on the hosts that the client targets, so following it would hang \
                     rather than fail",
                    redact(realm)
                ),
            ));
        }
        if !realm.starts_with("https://") {
            return Err(http(
                Class::Protocol,
                &what,
                format!("the token realm {} is not an absolute https URL", redact(realm)),
            ));
        }

        let mut url = realm.clone();
        let mut sep = if realm.contains('?') { '&' } else { '?' };
        if let Some(service) = params.get("service") {
            url.push_str(&format!("{sep}service={}", query_value(service)));
            sep = '&';
        }
        // The scope of the challenge when it gives one, else the scope of
        // this request: a registry that omits it still needs to know what the
        // token is for.
        let wanted = params.get("scope").map(String::as_str).unwrap_or(scope);
        url.push_str(&format!("{sep}scope={}", query_value(wanted)));

        // A stored login goes to the realm as `Basic`. No login: no
        // credential, as before. A failed helper refuses here, not a token
        // with no login behind a helper that failed.
        let token_of = format!("token from {}", redact(realm));
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(realm_host) = host_of(realm) {
            match self.logins.lookup(&realm_host) {
                Ok(Some(cred)) => headers.push(("Authorization", crate::credentials::basic_header(&cred))),
                Ok(None) => {}
                Err(refused) => {
                    return Err(Failure::caused(
                        refused.stage,
                        refused.class,
                        format!("{token_of}: {refused}"),
                        refused,
                    ));
                }
            }
        }

        #[derive(Deserialize)]
        struct TokenBody {
            token: Option<String>,
            access_token: Option<String>,
        }
        // The realm is checked whatever the policy of the registry: a token
        // is a credential, and only the registry was named insecure.
        let mut resp = self.net.send("GET", &url, &headers, true, LIMITS).map_err(|e| status_error(realm, e))?;
        let body = text(&mut resp, TOKEN_CEILING).map_err(|e| {
            let text = format!("{token_of}: reading the token response: {e}");
            Failure::caused(Stage::Connect, Class::of_io(e.kind()), text, e)
        })?;
        // The error names the failure, never the body: the body of a token
        // answer is the credential.
        let body: TokenBody = serde_json::from_str(&body)
            .map_err(|e| http(Class::Protocol, &token_of, format!("the token response does not parse: {e}")))?;
        // Neither branch prints the value. The two names are the two in
        // use; a registry that sends neither gave no token.
        body.token
            .or(body.access_token)
            .ok_or_else(|| http(Class::Protocol, &token_of, "the response carried neither `token` nor `access_token`"))
    }
}
