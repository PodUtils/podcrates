//! The read of the management API: a tunnel by its name or by its id and
//! cluster, with the scope `connect`, through the HTTP client of the
//! caller. The user's token goes in `Authorization` and nowhere else; a
//! failure names the method, the URL with no query and the status, and of
//! a refusal only the title and the detail of its problem document.

use std::fmt;

use pod_core::text::one_line;
use pod_core::{Class, Failure};
use pod_proxy::client::Request;

use crate::failure::{Step, fail, wrap};
use crate::info::{ANSWER_MOST, ConnectInfo};
use crate::service;
use crate::settings::{Base, Settings};
use crate::token::UserToken;

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

    /// The request of the connect information of `locator` with the token
    /// of `user`.
    fn request(&self, locator: &Locator, user: &UserToken) -> Request {
        Request::new("GET", self.url(locator))
            .header("Authorization", user.header().as_str())
            .header("Accept", "application/json")
            .timeout(self.timeout)
            .max_body(ANSWER_MOST)
    }

    /// The connect information of the tunnel `locator`, with a fresh
    /// connect token. A failure is of the step [`Step::Management`]; a
    /// refused user token (401, 403) is [`Class::Refused`] and is not tried
    /// again: the caller asks its user for a new one.
    pub async fn connect_info(&self, locator: &Locator, user: &UserToken) -> Result<ConnectInfo, Failure> {
        let request = self.request(locator, user);
        let shown = format!("GET {}{}/{locator}", self.base.url_of(&self.base.host), service::tunnels_path());
        let answer = self.client.send(request).await.map_err(|e| wrap(Step::Management, &shown, e))?;
        if !answer.is_success() {
            let words = problem(&answer.body);
            let what = if words.is_empty() {
                format!("{shown} answered HTTP {}", answer.status)
            } else {
                format!("{shown} answered HTTP {}: {words}", answer.status)
            };
            return Err(fail(Step::Management, class_of(answer.status), what));
        }
        ConnectInfo::parse(&answer.body, service::token_scope())
    }
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
}
