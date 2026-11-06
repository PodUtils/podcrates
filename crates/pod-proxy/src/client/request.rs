//! One request of [`super::Client::send`] and
//! [`super::Client::send_streamed`]: its method, URL, headers and body, its
//! time limits, the bound of its answer's body, and, for this request only,
//! a way out and a trust.

use std::sync::Arc;
use std::time::Duration;

use pod_core::Dial;
use pod_tls::Trust;

use super::url::Target;

/// The largest body of an answer, unless a request sets its own.
pub const MAX_BODY: usize = 16 << 20;

/// The largest head of an answer of [`super::Client::send`].
pub const MAX_HEAD: usize = 64 << 10;

/// The time limit of a request, unless it sets its own.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The longest wait for the next bytes of a streamed body, unless a request
/// sets its own.
pub const BODY_IDLE: Duration = Duration::from_secs(30);

/// One request.
#[derive(Clone)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The time limit of the whole request of [`super::Client::send`]; of
    /// the request and the head of the answer of
    /// [`super::Client::send_streamed`].
    pub timeout: Duration,
    /// The longest wait for the next bytes of the body of
    /// [`super::Client::send_streamed`]: a large body takes long, so no
    /// limit applies to the whole of it.
    pub body_idle: Duration,
    /// A way out for this request only, in place of the client's dial.
    pub via: Option<Arc<dyn Dial>>,
    /// The trust of `https://` for this request only; `None`: the roots of
    /// the client's TLS context ([`pod_tls::Context::roots`]).
    pub trust: Option<Trust>,
    /// The largest body of the answer of [`super::Client::send`]. A
    /// streamed body has no bound: its reader takes what it needs.
    pub max_body: usize,
}

impl std::fmt::Debug for Request {
    /// No header value and no body: they can hold a token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &super::url::without_query(&self.url))
            .field("headers", &self.headers.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .field("body", &self.body.as_ref().map(Vec::len))
            .field("timeout", &self.timeout)
            .field("body_idle", &self.body_idle)
            .field("via", &self.via.as_ref().map(|d| d.label()))
            .field("max_body", &self.max_body)
            .finish_non_exhaustive()
    }
}

impl Request {
    pub fn new(method: &'static str, url: impl Into<String>) -> Request {
        Request {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout: TIMEOUT,
            body_idle: BODY_IDLE,
            via: None,
            trust: None,
            max_body: MAX_BODY,
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Request {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// The body, as it is.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Request {
        self.body = Some(body.into());
        self
    }

    /// A JSON body, as its text (`value.to_string()`), with its
    /// `Content-Type`.
    pub fn json(self, text: impl Into<String>) -> Request {
        self.body(text.into().into_bytes()).header("Content-Type", "application/json")
    }

    pub fn timeout(mut self, timeout: Duration) -> Request {
        self.timeout = timeout;
        self
    }

    /// The longest wait for the next bytes of a streamed body, in place of
    /// [`BODY_IDLE`].
    pub fn body_idle(mut self, idle: Duration) -> Request {
        self.body_idle = idle;
        self
    }

    /// A way out for this request only, or the client's with `None`.
    pub fn via(mut self, dial: Option<Arc<dyn Dial>>) -> Request {
        self.via = dial;
        self
    }

    /// The trust of `https://` for this request only.
    pub fn trust(mut self, trust: Trust) -> Request {
        self.trust = Some(trust);
        self
    }

    /// Set the largest body of the answer, in place of [`MAX_BODY`].
    pub fn max_body(mut self, max: usize) -> Request {
        self.max_body = max;
        self
    }

    /// The head of the request to `target`: `Host` with the port when it
    /// is not the default of the scheme, `Connection: close`, `Accept-Encoding:
    /// identity`, the headers, `user_agent` when no header gives one and it
    /// is not empty, and `Content-Length` with a body or a method that has
    /// one. A header with a line break, or a `:` in its name, is refused.
    pub(crate) fn head(&self, target: &Target, user_agent: &str) -> Result<String, String> {
        let host = if target.host.contains(':') { format!("[{}]", target.host) } else { target.host.clone() };
        let default = if target.tls { 443 } else { 80 };
        let host = if target.port == default { host } else { format!("{host}:{}", target.port) };
        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept-Encoding: identity\r\n",
            self.method, target.path
        );
        let mut has_agent = false;
        for (name, value) in &self.headers {
            if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
                return Err("a request header has a line break".into());
            }
            has_agent |= name.eq_ignore_ascii_case("user-agent");
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !has_agent && !user_agent.is_empty() {
            if user_agent.contains(['\r', '\n']) {
                return Err("the user agent has a line break".into());
            }
            head.push_str(&format!("User-Agent: {user_agent}\r\n"));
        }
        let body = self.body.as_deref().unwrap_or_default();
        if self.body.is_some() || matches!(self.method, "POST" | "PUT" | "PATCH") {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        Ok(head)
    }
}
