//! The read of the management API: a tunnel by its name or by its id and
//! cluster, with the scope `connect`, through the HTTP client of the
//! caller. The caller's token goes in `Authorization` and nowhere else; a
//! caller with no token sends no `Authorization`. A failure names the
//! method, the URL with no query, the form of the credential and the
//! status, and of a refusal only the title and the detail of its problem
//! document.

use std::fmt;

use pod_core::text::one_line;
use pod_core::{Class, Failure};
use pod_proxy::client::Request;

use crate::failure::{Step, fail, wrap};
use crate::info::{ANSWER_MOST, ConnectInfo};
use crate::service;
use crate::settings::{Base, Settings};
use crate::token::{ConnectToken, Credential};

mod writes;
pub use writes::{Owned, OwnedPort, new_id};

/// The most characters of a refusal's words in a failure.
const WORDS_MOST: usize = 200;

/// A tunnel: by its name (the global host), or by its id and its cluster
/// (the host of the cluster).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Locator {
    Name(String),
    Id { id: String, cluster: String },
}

/// A label of a name: 1 to 63 lower-case letters, digits and `-`, with no
/// `-` at an end, as a part of a host name.
fn label(text: &str) -> bool {
    (1..=63).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !text.starts_with('-')
        && !text.ends_with('-')
}

impl Locator {
    /// `NAME`, or `ID.CLUSTER` as the vendor's tools show a tunnel. The
    /// text is lower-cased; each part is a label of a host name, so that it
    /// goes in a path and a host as it is.
    pub fn parse(text: &str) -> Result<Locator, Failure> {
        let lower = text.trim().to_ascii_lowercase();
        let bad = || {
            fail(Step::Management, Class::Usage, format!("{:?} is no tunnel name and no ID.CLUSTER", one_line(text)))
        };
        match lower.split_once('.') {
            None if label(&lower) => Ok(Locator::Name(lower)),
            Some((id, cluster)) if label(id) && label(cluster) => {
                Ok(Locator::Id { id: id.to_string(), cluster: cluster.to_string() })
            }
            _ => Err(bad()),
        }
    }

    /// The last part of the path of the tunnel.
    fn ident(&self) -> &str {
        match self {
            Locator::Name(name) => name,
            Locator::Id { id, .. } => id,
        }
    }
}

impl fmt::Display for Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Locator::Name(name) => f.write_str(name),
            Locator::Id { id, cluster } => write!(f, "{id}.{cluster}"),
        }
    }
}

/// The class of an answer of the management API with `status`.
fn class_of(status: u16) -> Class {
    match status {
        401 | 403 => Class::Refused,
        404 => Class::NotFound,
        429 => Class::TempFail,
        400..=499 => Class::Protocol,
        _ => Class::Unavailable,
    }
}

/// The title and the detail of a problem document (RFC 9457), one line,
/// cut: the words of a refusal with no other byte of its body.
fn problem(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Problem {
        title: Option<String>,
        detail: Option<String>,
    }
    let Ok(problem) = serde_json::from_slice::<Problem>(body) else { return String::new() };
    let words: Vec<String> = [problem.title, problem.detail].into_iter().flatten().map(|w| one_line(&w)).collect();
    words.join(": ").chars().take(WORDS_MOST).collect()
}

/// The management API of a caller: its HTTP client and its settings.
#[derive(Clone, Debug)]
pub struct Management {
    client: pod_proxy::Client,
    base: Base,
    api_version: String,
    timeout: std::time::Duration,
}

impl Management {
    /// The API at the base of `settings`, through `client`.
    pub fn new(client: pod_proxy::Client, settings: &Settings) -> Management {
        Management {
            client,
            base: settings.management.clone(),
            api_version: settings.api_version.clone(),
            timeout: settings.timeout,
        }
    }

    /// The URL of the tunnel, with the scope `connect` and the version:
    /// the global host for a name, the host of its cluster for an id.
    pub fn url(&self, locator: &Locator) -> String {
        let host = match locator {
            Locator::Name(_) => self.base.host.clone(),
            Locator::Id { cluster, .. } => self.base.cluster_host(cluster),
        };
        format!(
            "{}{}/{}?tokenScopes={}&api-version={}",
            self.base.url_of(&host),
            service::tunnels_path(),
            locator.ident(),
            service::token_scope(),
            self.api_version
        )
    }

    /// The request of the connect information of `locator` with
    /// `credential`.
    fn request(&self, locator: &Locator, credential: &Credential) -> Request {
        let request = Request::new("GET", self.url(locator));
        let request = match credential.header() {
            Some(value) => request.header("Authorization", value.as_str()),
            None => request,
        };
        request.header("Accept", "application/json").timeout(self.timeout).max_body(ANSWER_MOST)
    }

    /// The connect information of the tunnel `locator`, with a fresh
    /// connect token when the answer holds one. A failure is of the step
    /// [`Step::Management`] and names the form of `credential`; a refusal
    /// (401, 403) is [`Class::Refused`] and is not tried again: the caller
    /// asks its user for a new token, or for one where it gave none.
    pub async fn connect_info(&self, locator: &Locator, credential: &Credential) -> Result<ConnectInfo, Failure> {
        let request = self.request(locator, credential);
        let shown = format!(
            "GET {}{}/{locator} with {}",
            self.base.url_of(&self.base.host),
            service::tunnels_path(),
            credential.form()
        );
        let line = request.line();
        let answer = self.client.send(request).await.map_err(|e| wrap(Step::Management, &line, e))?;
        if !answer.is_success() {
            let words = problem(&answer.body);
            let what = if words.is_empty() {
                format!("{shown} answered HTTP {}", answer.status)
            } else {
                format!("{shown} answered HTTP {}: {words}", answer.status)
            };
            return Err(fail(Step::Management, class_of(answer.status), what));
        }
        relay_token(ConnectInfo::parse(&answer.body, service::token_scope())?, credential)
    }
}

/// The token of the relay for an answer with no connect token, by the form
/// of the credential: a user token is refused (the user has no connect
/// access); a tunnel access token goes to the relay itself, as the vendor's
/// TypeScript client keeps the caller's token (`preserveAccessTokens`); with
/// no token the upgrade carries none, as that client sends no
/// `Authorization` with no token.
fn relay_token(mut info: ConnectInfo, credential: &Credential) -> Result<ConnectInfo, Failure> {
    if info.token().is_some() {
        return Ok(info);
    }
    match credential {
        Credential::User(_) => {
            return Err(fail(
                Step::Management,
                Class::Refused,
                format!(
                    "the answer for {} to {} holds no token of the scope {:?}",
                    info.tunnel.shown(),
                    credential.form(),
                    service::token_scope()
                ),
            ));
        }
        Credential::Access(access) => info.take_caller(ConnectToken::of_access(access)),
        Credential::Anonymous => {}
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locator_is_a_name_or_an_id_with_its_cluster() {
        assert_eq!(Locator::parse("My-Box").unwrap(), Locator::Name("my-box".into()));
        let id = Locator::parse("bcd123.usw2").unwrap();
        assert_eq!(id, Locator::Id { id: "bcd123".into(), cluster: "usw2".into() });
        assert_eq!(id.to_string(), "bcd123.usw2");
        for bad in ["", "a/b", "a..b", "-a", "a.b.c", "a b", "../x", "a?x=1", &"x".repeat(64)] {
            assert_eq!(Locator::parse(bad).unwrap_err().class, Class::Usage, "{bad}");
        }
    }

    #[test]
    fn a_problem_document_gives_its_words_alone() {
        let body =
            br#"{"type":"x","title":"Unauthorized","detail":"The token\nexpired.","accessTokens":{"connect":"t"}}"#;
        assert_eq!(problem(body), "Unauthorized: The token expired.");
        assert_eq!(problem(b"<html>"), "");
        assert_eq!(class_of(401), Class::Refused);
        assert_eq!(class_of(404), Class::NotFound);
        assert_eq!(class_of(502), Class::Unavailable);
    }

    #[test]
    fn an_answer_with_no_connect_token_is_settled_by_the_form_of_the_credential() {
        use crate::token::{AccessToken, Scheme, UserToken};
        let body = br#"{"name":"a","endpoints":[
            {"connectionMode":"TunnelRelay","hostId":"h1","clientRelayUri":"wss://relay.example/c"}]}"#;
        let info = || ConnectInfo::parse(body, "connect").unwrap();
        let user = Credential::User(UserToken::new(Scheme::Github, "made-up-user-token").unwrap());
        let failure = relay_token(info(), &user).unwrap_err();
        assert_eq!(failure.class, Class::Refused);
        assert!(failure.chain().contains("to a GitHub user token holds no token"), "{}", failure.chain());
        let access = Credential::Access(AccessToken::new("made-up-access-token").unwrap());
        let given = relay_token(info(), &access).unwrap();
        assert_eq!(given.token().unwrap().header().as_str(), "tunnel made-up-access-token");
        assert_eq!(given.relay_form(), "the caller's tunnel access token");
        let none = relay_token(info(), &Credential::Anonymous).unwrap();
        assert!(none.token().is_none());
        assert_eq!(none.relay_form(), "no token");
    }
}
