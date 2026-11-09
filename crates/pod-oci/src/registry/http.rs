use super::*;

impl Client {
    /// One GET, with the bearer dance and the bounded retry around it.
    pub(super) fn get(
        &mut self,
        path: &str,
        endpoint: &str,
        scope: &str,
        accept: Option<&str>,
    ) -> Result<Body> {
        let mut last: Option<Error> = None;
        let mut target = format!("{}{path}", self.base(endpoint));
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(backoff(attempt, None));
            }
            match self.get_once(&target, endpoint, scope, accept) {
                Ok(Attempt::Done(r)) => return Ok(*r),
                Ok(Attempt::Retry {
                    after,
                    why,
                    transport_host,
                }) => {
                    // ⭐ TODO/image.md T-0213. A transport failure against an
                    // endpoint the CALLER named insecure is the one case where
                    // podvm retries over plain HTTP, and it says so.
                    // ⛔ Never a discovery: `after_connect_failure` answers
                    // `None` for every endpoint nobody named, which keeps
                    // T-0201's finding intact. An automatic downgrade on a
                    // network where tcp/80 is black-holed is a hang.
                    if transport_host.is_some()
                        && !self.http_endpoints.contains(endpoint)
                        && let Some(d) = self.policy.after_connect_failure(endpoint)
                    {
                        self.http_endpoints.insert(endpoint.to_string());
                        let _ = writeln!(
                            std::io::stderr(),
                            "podvm: {endpoint} did not answer over HTTPS ({why}), and it \
                                 is configured as an insecure registry, so this and every \
                                 later request to it use {}://",
                            d.scheme
                        );
                        target = format!("{}{}", self.base(endpoint), path);
                        continue;
                    }
                    if attempt + 1 < ATTEMPTS {
                        std::thread::sleep(backoff(attempt + 1, after));
                    }
                    last = Some(Error::Http {
                        // ⭐ TODO/image.md T-1420. A transport failure names its
                        // host once, in `detail`. Repeating the URL here is the
                        // `GET url: transport: url: ...` shape this entry
                        // removes; the path says which call failed.
                        what: match &transport_host {
                            Some(_) => format!("GET {}", redact(path)),
                            None => format!("GET {}", redact(&target)),
                        },
                        detail: match transport_host {
                            Some(host) => format!("{host}: {why}"),
                            None => why,
                        },
                    });
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| Error::Http {
            what: format!("GET {}", redact(&target)),
            detail: format!("gave up after {ATTEMPTS} attempt(s)"),
        }))
    }

    fn get_once(
        &mut self,
        url: &str,
        endpoint: &str,
        scope: &str,
        accept: Option<&str>,
    ) -> Result<Attempt> {
        let verify = self.policy.verify(endpoint);
        match self.send(url, scope, accept, None, verify) {
            Ok(r) => Ok(Attempt::Done(Box::new(r))),
            Err(Refusal::Status(resp)) if resp.status() == 401 => {
                // The challenge decides where the token comes from. ⚠ Read from
                // the response rather than hard-coded, so a registry that is not
                // Docker Hub works without a table of hosts.
                let Some(challenge) = resp.header("WWW-Authenticate").map(str::to_string) else {
                    return Err(Error::Http {
                        what: format!("GET {}", redact(url)),
                        detail: "401 with no WWW-Authenticate header, so there is no \
                                 challenge to answer"
                            .into(),
                    });
                };
                // TODO/image.md T-0209: a Basic challenge is answered from the
                // stored logins, never anonymously retried. An anonymous retry
                // after a Basic 401 fails the same way and teaches nothing.
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
            Err(Refusal::Status(resp)) if retryable(resp.status()) => Ok(Attempt::Retry {
                after: resp.header("Retry-After").and_then(parse_retry_after),
                why: format!("HTTP {}", resp.status()),
                transport_host: None,
            }),
            Err(Refusal::Transport(t)) => Ok(Attempt::Retry {
                after: None,
                transport_host: Some(transport_host(url, &t)),
                why: transport_why(&t),
            }),
            Err(e) => Err(status_error(url, e)),
        }
    }

    /// One GET through the transport of this client, with the trust that
    /// the policy gives the endpoint.
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
        // ⚠ Keyed by the URL's own host: a redirect to a CDN must not be sent
        // the registry's bearer token, and `net` does not carry the
        // `Authorization` header to another host. The token is attached to the
        // request this client builds, which is always the registry.
        //
        // ⚠ A Basic answer rides instead of the token, never beside it: the
        // 401 that produced it refused the Bearer the first request carried,
        // and re-sending a refused credential beside the new one is how a
        // secret reaches a log that records rejected attempts.
        if let Some(b) = basic {
            headers.push(("Authorization", b.to_string()));
        } else if let Some(host) = host_of(url)
            && let Some(t) = self.tokens.get(&key(&host, scope))
        {
            headers.push(("Authorization", format!("Bearer {t}")));
        }
        self.net()?.send("GET", url, &headers, verify, LIMITS)
    }

    /// Answer a `Basic` challenge from the stored logins.
    ///
    /// `Ok(None)` from the lookup is a named refusal, not an anonymous
    /// retry: the registry just refused the anonymous request. A failed
    /// helper refuses by name through the lookup's error.
    fn answer_basic(
        &mut self,
        url: &str,
        scope: &str,
        accept: Option<&str>,
        challenge: &str,
        verify: bool,
    ) -> Result<Attempt> {
        let host = host_of(url).unwrap_or_else(|| "<unknown>".into());
        let cred = match crate::credentials::lookup(&host) {
            Ok(Some(cred)) => cred,
            Ok(None) => {
                return Err(Error::Http {
                    what: format!("GET {}", redact(url)),
                    detail: format!(
                        "the registry asked for Basic authentication (challenge {challenge:?}), \
                         and no login is stored for {host} (TODO/image.md T-0209)"
                    ),
                });
            }
            Err(refused) => {
                return Err(Error::Http {
                    what: format!("GET {}", redact(url)),
                    detail: refused,
                });
            }
        };
        let header = crate::credentials::basic_header(&cred);
        match self.send(url, scope, accept, Some(&header), verify) {
            Ok(r) => Ok(Attempt::Done(Box::new(r))),
            Err(e) => Err(status_error(url, e)),
        }
    }

    /// Answer a `Bearer` challenge at the realm it names.
    fn token(&mut self, endpoint: &str, scope: &str, challenge: &str) -> Result<String> {
        let params = parse_challenge(challenge);
        let scheme = challenge.split_whitespace().next().unwrap_or("");
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(Error::Http {
                what: format!("authenticating to {endpoint}"),
                detail: format!(
                    "the registry asked for {scheme:?} authentication, and podvm \
                     implements Bearer only"
                ),
            });
        }
        let Some(realm) = params.get("realm") else {
            return Err(Error::Http {
                what: format!("authenticating to {endpoint}"),
                detail: "the Bearer challenge names no realm".into(),
            });
        };
        if realm.starts_with("http://") {
            // ⚠ NOT `PlainHttpRefused`: that variant means the caller wrote an
            // `http://` reference and exits 2. This is the registry naming a
            // plain-HTTP realm, which is the same refusal about somebody
            // else's input and is a runtime failure, not a usage one.
            return Err(Error::Http {
                what: format!("authenticating to {endpoint}"),
                detail: format!(
                    "the token realm is {}, and podvm speaks HTTPS only. tcp/80 \
                     egress is broken on the runtime podvm targets, so following \
                     it would hang rather than fail (TODO/image.md T-0201)",
                    redact(realm)
                ),
            });
        }
        if !realm.starts_with("https://") {
            return Err(Error::Http {
                what: format!("authenticating to {endpoint}"),
                detail: format!(
                    "the token realm {} is not an absolute https URL",
                    redact(realm)
                ),
            });
        }

        let mut url = realm.clone();
        let mut sep = if realm.contains('?') { '&' } else { '?' };
        if let Some(service) = params.get("service") {
            url.push_str(&format!(
                "{sep}service={}",
                crate::net::query_value(service)
            ));
            sep = '&';
        }
        // ⚠ The challenge's own scope when it gives one, and the scope this
        // request needs otherwise. A registry that omits it still has to be
        // told what the token is for.
        let wanted = params.get("scope").map(String::as_str).unwrap_or(scope);
        url.push_str(&format!("{sep}scope={}", crate::net::query_value(wanted)));

        // TODO/image.md T-0209: a stored login rides to the token realm as
        // Basic. No login stays anonymous, exactly as before; a failed
        // helper refuses here rather than minting an anonymous token behind
        // a delegation that just failed.
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(realm_host) = host_of(realm) {
            match crate::credentials::lookup(&realm_host) {
                Ok(Some(cred)) => {
                    headers.push(("Authorization", crate::credentials::basic_header(&cred)));
                }
                Ok(None) => {}
                Err(refused) => {
                    return Err(Error::Http {
                        what: format!("token from {}", redact(realm)),
                        detail: refused,
                    });
                }
            }
        }

        #[derive(Deserialize)]
        struct TokenBody {
            token: Option<String>,
            access_token: Option<String>,
        }
        // ⚠ The realm is verified whatever the registry's policy: a token is a
        // credential, and only the registry was named insecure.
        let mut resp = self
            .net()
            .map_err(|e| status_error(realm, e))?
            .send("GET", &url, &headers, true, LIMITS)
            .map_err(|e| status_error(realm, e))?;
        // ⚠ The text and `serde_json`, which TODO/deps.md T-0908 already
        // pinned and measured, rather than a second JSON reader. It is bounded
        // at 10 MiB, which a token response is far under.
        let text = crate::net::text(&mut resp, TOKEN_CEILING).map_err(|e| Error::Http {
            what: format!("token from {}", redact(realm)),
            detail: format!("reading the token response: {e}"),
        })?;
        // ⛔ The error names the failure and never the body: a token response's
        // body IS the credential. docs/security/secrets.md.
        let body: TokenBody = serde_json::from_str(&text).map_err(|e| Error::Http {
            what: format!("token from {}", redact(realm)),
            detail: format!("the token response does not parse: {e}"),
        })?;
        // ⛔ Neither branch prints the value. The two field names are the two
        // spellings in use; a registry sending neither has not issued a token.
        body.token.or(body.access_token).ok_or_else(|| Error::Http {
            what: format!("token from {}", redact(realm)),
            detail: "the response carried neither `token` nor `access_token`".into(),
        })
    }
}
