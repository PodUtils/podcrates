//! One file of a release over HTTPS (feature `fetch`), through the
//! caller's `pod_proxy::Client`: its way out (a proxy), its TLS and its
//! user agent, as each connection of the consumer goes. A GET follows its
//! redirects to at most [`REDIRECTS`] hops and on HTTPS alone, and its body
//! is bounded in size and time. GitHub sends an asset to a signed URL of
//! another host; that URL carries a credential of the moment in its query,
//! so no message names more of a URL than its host and its path.
//!
//! The base is podssh's `update/fetch.rs`; its URL and its redirects are
//! the client's of `pod-proxy` now.

use std::time::Duration;

use pod_proxy::Client;
use pod_proxy::client::Request;

use crate::Error;

/// The most redirects that one file follows: GitHub's asset takes one.
pub const REDIRECTS: u8 = 5;

/// Where and how a release's files are fetched.
#[derive(Clone, Debug)]
pub struct Fetcher {
    pub client: Client,
    /// Refuse a URL that is not `https://`. Only a test against a stand-in
    /// on the loopback turns it off ([`Fetcher::allow_plain_http`]).
    pub https_only: bool,
}

/// `url` with no query and no fragment: what a message may say.
pub fn shown(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

impl Fetcher {
    /// A fetcher over `client`, HTTPS only.
    pub fn new(client: Client) -> Fetcher {
        Fetcher { client, https_only: true }
    }

    /// Take `http://` URLs too: for a stand-in on the loopback.
    pub fn allow_plain_http(mut self) -> Fetcher {
        self.https_only = false;
        self
    }

    /// The body of `url`, of `max` bytes at most, within `limit` for the
    /// whole of it, redirects included. A 404 is [`Error::NotFound`].
    pub async fn get(&self, url: &str, accept: &str, max: usize, limit: Duration) -> Result<Vec<u8>, Error> {
        let at = shown(url);
        if self.https_only && !url.starts_with("https://") {
            return Err(Error::Config(format!("{at}: not an https:// URL")));
        }
        let request =
            Request::new("GET", url).header("Accept", accept).redirects(REDIRECTS).max_body(max).timeout(limit);
        let answer = self.client.send(request).await.map_err(|e| Error::Unavailable(e.chain()))?;
        let last = shown(&answer.url);
        let last = if last.is_empty() { at } else { last };
        match answer.status {
            200 => Ok(answer.body),
            404 => Err(Error::NotFound(format!("{last}: not found (HTTP 404)"))),
            status => Err(Error::Unavailable(format!(
                "{last}: HTTP {status} {}: {}",
                pod_core::text::one_line(&answer.reason),
                answer.body_text(120)
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_message_shows_holds_no_query() {
        assert_eq!(
            shown("https://release-assets.githubusercontent.com/a/b?sig=SECRET&exp=1"),
            "https://release-assets.githubusercontent.com/a/b"
        );
        assert_eq!(shown("https://github.com/o/r#x"), "https://github.com/o/r");
    }
}
